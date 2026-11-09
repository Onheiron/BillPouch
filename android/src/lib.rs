use axum::body::Body;
use bp_core::{
    config,
    control::{client, protocol::ControlRequest, protocol::ControlResponse},
    identity::Identity,
    service::ServiceType,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_fs::{FsExt, OpenOptions};
use tower::ServiceExt;

static DAEMON: AtomicBool = AtomicBool::new(false);

#[derive(Serialize)]
struct AppState {
    identity: bool,
    running: bool,
    member: bool,
}

#[derive(Serialize)]
struct ApiReply {
    status: u16,
    body: String,
}

async fn control(req: ControlRequest) -> Result<serde_json::Value, String> {
    let socket = config::socket_path().map_err(|e| e.to_string())?;
    match client::send(&socket, &req)
        .await
        .map_err(|e| e.to_string())?
    {
        ControlResponse::Ok { data } => Ok(data.unwrap_or_default()),
        ControlResponse::Error { message } => Err(message),
    }
}

async fn start_daemon(passphrase: Option<String>) -> Result<(), String> {
    if bp_core::daemon::is_running() {
        DAEMON.store(true, Ordering::SeqCst);
        return Ok(());
    }
    Identity::load(passphrase.as_deref()).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn(async move {
        if let Err(e) = bp_core::daemon::run_daemon(passphrase).await {
            eprintln!("BillPouch daemon stopped: {e}");
        }
        DAEMON.store(false, Ordering::SeqCst);
    });
    for _ in 0..100 {
        if bp_core::daemon::is_running() {
            DAEMON.store(true, Ordering::SeqCst);
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err("the node did not start".into())
}

#[tauri::command]
async fn app_state() -> AppState {
    let identity = Identity::exists().unwrap_or(false);
    let running = DAEMON.load(Ordering::SeqCst) && bp_core::daemon::is_running();
    let member = running
        && control(ControlRequest::Status)
            .await
            .ok()
            .and_then(|s| s["networks"].as_array().map(|n| !n.is_empty()))
            .unwrap_or(false);
    AppState {
        identity,
        running,
        member,
    }
}

#[tauri::command]
async fn unlock(passphrase: String) -> Result<(), String> {
    start_daemon(Some(passphrase).filter(|p| !p.is_empty())).await
}

#[tauri::command]
async fn pick_key(app: AppHandle) -> Result<Option<String>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_file()
            .map(|path| app.fs().read_to_string(path))
    })
    .await
    .map_err(|e| e.to_string())?;
    picked.transpose().map_err(|e| e.to_string())
}

#[tauri::command]
async fn setup(
    app: AppHandle,
    alias: String,
    key: String,
    passphrase: String,
    token: String,
) -> Result<(), String> {
    let passphrase = Some(passphrase).filter(|p| !p.is_empty());
    if !Identity::exists().map_err(|e| e.to_string())? {
        let scratch = app
            .path()
            .app_cache_dir()
            .map_err(|e| e.to_string())?
            .join("user_key");
        std::fs::create_dir_all(scratch.parent().unwrap_or(&scratch)).map_err(|e| e.to_string())?;
        std::fs::write(&scratch, key.trim().to_string() + "\n").map_err(|e| e.to_string())?;
        let created = Identity::create(
            Some(&scratch),
            Some(alias).filter(|a| !a.trim().is_empty()),
            passphrase.as_deref(),
        );
        let _ = std::fs::remove_file(&scratch);
        created.map_err(|e| e.to_string())?;
    }
    start_daemon(passphrase).await?;
    let joined = control(ControlRequest::JoinNetwork {
        token: token.trim().to_string(),
    })
    .await?;
    let network_id = joined["network_id"]
        .as_str()
        .ok_or("the token did not name a network")?
        .to_string();
    control(ControlRequest::Hatch {
        service_type: ServiceType::Bill,
        network_id,
        metadata: HashMap::new(),
    })
    .await
    .map(|_| ())
}

#[tauri::command]
async fn api(method: String, path: String, body: Option<String>) -> Result<ApiReply, String> {
    let socket = config::socket_path().map_err(|e| e.to_string())?;
    let request = axum::http::Request::builder()
        .method(method.as_str())
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.unwrap_or_default()))
        .map_err(|e| e.to_string())?;
    let response = bp_api::router(socket, None)
        .oneshot(request)
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ApiReply {
        status,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

fn scratch(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("transfer");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(format!("{:016x}", rand_suffix())))
}

fn rand_suffix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default()
}

fn display_name(path: &tauri_plugin_fs::FilePath, n: usize) -> String {
    let text = path.to_string();
    let last = text
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default()
        .replace("%20", " ");
    let decoded = last
        .rsplit("%2F")
        .next()
        .unwrap_or_default()
        .rsplit("%3A")
        .next()
        .unwrap_or_default()
        .to_string();
    if decoded.is_empty() {
        format!("file-{n}")
    } else {
        decoded
    }
}

#[tauri::command]
async fn upload_files(app: AppHandle, network: String) -> Result<Vec<String>, String> {
    let picker = app.clone();
    let picked =
        tauri::async_runtime::spawn_blocking(move || picker.dialog().file().blocking_pick_files())
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
    let mut stored = Vec::new();
    for (n, path) in picked.into_iter().enumerate() {
        let name = display_name(&path, n + 1);
        let bytes = app.fs().read(path).map_err(|e| format!("{name}: {e}"))?;
        let temp = scratch(&app)?;
        std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
        let result = control(ControlRequest::PutFile {
            path: temp.clone(),
            ph: None,
            network_id: network.clone(),
            file_name: Some(name.clone()),
        })
        .await;
        let _ = std::fs::remove_file(&temp);
        result.map_err(|e| format!("{name}: {e}"))?;
        stored.push(name);
    }
    Ok(stored)
}

#[tauri::command]
async fn save_file(
    app: AppHandle,
    file_id: String,
    network: String,
    name: String,
) -> Result<bool, String> {
    let temp = scratch(&app)?;
    control(ControlRequest::GetFile {
        file_id,
        network_id: network,
        output: temp.clone(),
    })
    .await?;
    let chooser = app.clone();
    let target = tauri::async_runtime::spawn_blocking(move || {
        chooser
            .dialog()
            .file()
            .set_file_name(name)
            .blocking_save_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(target) = target else {
        let _ = std::fs::remove_file(&temp);
        return Ok(false);
    };
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    let copied = app
        .fs()
        .open(target, options)
        .and_then(|mut out| std::io::copy(&mut std::fs::File::open(&temp)?, &mut out));
    let _ = std::fs::remove_file(&temp);
    copied.map(|_| true).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let home = app.path().app_data_dir()?;
            let run = home.join("run");
            std::fs::create_dir_all(&run)?;
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_RUNTIME_DIR", &run);
            std::env::set_var("BP_LISTEN_PORT", "off");
            if Identity::exists().unwrap_or(false) {
                tauri::async_runtime::spawn(async {
                    let _ = start_daemon(None).await;
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_state,
            unlock,
            pick_key,
            setup,
            api,
            upload_files,
            save_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running BillPouch");
}
