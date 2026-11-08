use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use base64::Engine as _;
use bp_core::{
    config,
    control::{
        client,
        protocol::{ControlRequest, ControlResponse, GetFileData},
    },
    service::ServiceType,
};
use clap::Parser;
use futures::StreamExt;
use serde::Deserialize;
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::instrument;

#[derive(Parser)]
#[command(
    name = "bp-api",
    about = "🦤 BillPouch REST API gateway",
    version,
    long_about = None
)]
struct Args {
    #[arg(help = "TCP port to listen on")]
    #[arg(long, default_value_t = 3000)]
    port: u16,

    #[arg(help = "Host address to bind")]
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

#[derive(Clone)]
struct AppState {
    socket_path: Arc<PathBuf>,
    user_key: Arc<String>,
}

fn daemon_user_key() -> anyhow::Result<String> {
    let cert: bp_core::identity::DeviceCert =
        serde_json::from_slice(&std::fs::read(config::device_cert_path()?)?)?;
    Ok(cert.user_key)
}

fn load_or_create_tls() -> anyhow::Result<(PathBuf, PathBuf, String)> {
    let dir = config::base_dir()?;
    let (cert_path, key_path) = (dir.join("api_cert.pem"), dir.join("api_key.pem"));
    if !cert_path.exists() || !key_path.exists() {
        std::fs::create_dir_all(&dir)?;
        let generated =
            rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()])?;
        std::fs::write(&cert_path, generated.cert.pem())?;
        std::fs::write(&key_path, generated.key_pair.serialize_pem())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    let pem = std::fs::read_to_string(&cert_path)?;
    let der: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(der)?;
    let digest = <sha2::Sha256 as sha2::Digest>::digest(&der);
    let fingerprint = digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    Ok((cert_path, key_path, fingerprint))
}

async fn require_session(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let presented = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Session "))
        .unwrap_or("");
    let now = chrono_now();
    match bp_core::session::verify(presented, &state.user_key, now) {
        Ok(()) => next.run(request).await,
        Err(e) => api_error(
            StatusCode::UNAUTHORIZED,
            format!("{e} — open the dashboard with `bp dashboard`"),
        ),
    }
}

fn chrono_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn call_daemon(
    state: &AppState,
    req: ControlRequest,
) -> Result<serde_json::Value, Box<Response>> {
    match client::send(&state.socket_path, &req).await {
        Err(e) => Err(Box::new(daemon_unavailable(e))),
        Ok(ControlResponse::Ok { data }) => Ok(data.unwrap_or(serde_json::Value::Null)),
        Ok(ControlResponse::Error { message }) => Err(Box::new(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            message,
        ))),
    }
}

async fn respond(state: &AppState, req: ControlRequest, status: StatusCode) -> Response {
    match call_daemon(state, req).await {
        Ok(data) => (status, Json(data)).into_response(),
        Err(resp) => *resp,
    }
}

fn daemon_unavailable(e: impl std::fmt::Display) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        format!("Daemon unavailable: {e}. Start with `bp hatch <type>`."),
    )
}

fn api_error(status: StatusCode, message: String) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

fn asset(content_type: &'static str, body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

async fn index() -> Response {
    asset(
        "text/html; charset=utf-8",
        include_str!("../static/index.html"),
    )
}

async fn app_js() -> Response {
    asset(
        "text/javascript; charset=utf-8",
        include_str!("../static/app.js"),
    )
}

async fn app_css() -> Response {
    asset("text/css; charset=utf-8", include_str!("../static/app.css"))
}

async fn service_worker() -> Response {
    asset(
        "text/javascript; charset=utf-8",
        include_str!("../static/sw.js"),
    )
}

async fn manifest() -> Response {
    asset(
        "application/manifest+json",
        include_str!("../static/manifest.webmanifest"),
    )
}

async fn icon() -> Response {
    asset("image/svg+xml", include_str!("../static/icon.svg"))
}

#[instrument(skip_all)]
async fn ping(State(state): State<AppState>) -> Response {
    respond(&state, ControlRequest::Ping, StatusCode::OK).await
}

#[instrument(skip_all)]
async fn status(State(state): State<AppState>) -> Response {
    respond(&state, ControlRequest::Status, StatusCode::OK).await
}

#[instrument(skip_all)]
async fn peers(State(state): State<AppState>) -> Response {
    respond(&state, ControlRequest::Flock, StatusCode::OK).await
}

#[derive(Deserialize)]
struct HatchBody {
    service_type: ServiceType,
    network_id: String,
    #[serde(default)]
    metadata: HashMap<String, serde_json::Value>,
}

#[instrument(skip_all)]
async fn hatch(State(state): State<AppState>, Json(body): Json<HatchBody>) -> Response {
    let req = ControlRequest::Hatch {
        service_type: body.service_type,
        network_id: body.network_id,
        metadata: body.metadata,
    };
    respond(&state, req, StatusCode::CREATED).await
}

#[derive(Deserialize, Default)]
struct FarewellQuery {
    #[serde(default)]
    evict: bool,
}

#[instrument(skip_all)]
async fn farewell(
    State(state): State<AppState>,
    Path(service_id): Path<String>,
    Query(query): Query<FarewellQuery>,
) -> Response {
    if query.evict {
        respond(
            &state,
            ControlRequest::FarewellEvict { service_id },
            StatusCode::OK,
        )
        .await
    } else {
        respond(
            &state,
            ControlRequest::Farewell { service_id },
            StatusCode::OK,
        )
        .await
    }
}

#[derive(Deserialize)]
struct PauseBody {
    eta_minutes: u64,
}

#[instrument(skip_all)]
async fn pause_service(
    State(state): State<AppState>,
    Path(service_id): Path<String>,
    Json(body): Json<PauseBody>,
) -> Response {
    respond(
        &state,
        ControlRequest::Pause {
            service_id,
            eta_minutes: body.eta_minutes,
        },
        StatusCode::OK,
    )
    .await
}

#[instrument(skip_all)]
async fn resume_service(State(state): State<AppState>, Path(service_id): Path<String>) -> Response {
    respond(
        &state,
        ControlRequest::Resume { service_id },
        StatusCode::OK,
    )
    .await
}

#[derive(Deserialize)]
struct NetworkBody {
    network_id: String,
    #[serde(default)]
    force: bool,
}

#[derive(Deserialize)]
struct CreateNetworkBody {
    name: String,
}

#[instrument(skip_all)]
async fn network_create(
    State(state): State<AppState>,
    Json(body): Json<CreateNetworkBody>,
) -> Response {
    respond(
        &state,
        ControlRequest::CreateNetwork { name: body.name },
        StatusCode::CREATED,
    )
    .await
}

#[derive(Deserialize)]
struct InviteBody {
    member_key: String,
}

#[instrument(skip_all)]
async fn network_invite(
    State(state): State<AppState>,
    Path(network_id): Path<String>,
    Json(body): Json<InviteBody>,
) -> Response {
    respond(
        &state,
        ControlRequest::Invite {
            network_id,
            member_key: body.member_key,
        },
        StatusCode::CREATED,
    )
    .await
}

#[derive(Deserialize)]
struct JoinBody {
    token: String,
}

#[instrument(skip_all)]
async fn network_join(State(state): State<AppState>, Json(body): Json<JoinBody>) -> Response {
    respond(
        &state,
        ControlRequest::JoinNetwork { token: body.token },
        StatusCode::OK,
    )
    .await
}

#[instrument(skip_all)]
async fn network_members(
    State(state): State<AppState>,
    Path(network_id): Path<String>,
) -> Response {
    respond(
        &state,
        ControlRequest::Members { network_id },
        StatusCode::OK,
    )
    .await
}

#[derive(Deserialize)]
struct ShareBody {
    network_id: String,
    member: String,
}

#[instrument(skip_all)]
async fn share_file(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    Json(body): Json<ShareBody>,
) -> Response {
    respond(
        &state,
        ControlRequest::Share {
            network_id: body.network_id,
            file_id,
            member: body.member,
        },
        StatusCode::OK,
    )
    .await
}

#[derive(Deserialize)]
struct ExpelBody {
    member: String,
}

#[instrument(skip_all)]
async fn network_expel(
    State(state): State<AppState>,
    Path(network_id): Path<String>,
    Json(body): Json<ExpelBody>,
) -> Response {
    respond(
        &state,
        ControlRequest::Expel {
            network_id,
            member: body.member,
        },
        StatusCode::OK,
    )
    .await
}

#[instrument(skip_all)]
async fn network_leave(State(state): State<AppState>, Json(body): Json<NetworkBody>) -> Response {
    respond(
        &state,
        ControlRequest::Leave {
            network_id: body.network_id,
            force: body.force,
        },
        StatusCode::OK,
    )
    .await
}

#[derive(Deserialize)]
struct PutFileQuery {
    network: String,
    name: Option<String>,
    ph: Option<f64>,
}

fn scratch_file() -> std::io::Result<PathBuf> {
    let dir = config::base_dir()
        .map_err(|e| std::io::Error::other(e.to_string()))?
        .join("api");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(hex::encode(rand::random::<[u8; 8]>())))
}

async fn save_body(body: Body, path: &std::path::Path) -> std::io::Result<()> {
    let mut file = tokio::fs::File::create(path).await?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        file.write_all(&chunk.map_err(std::io::Error::other)?)
            .await?;
    }
    file.flush().await
}

#[instrument(skip_all)]
async fn put_file(
    State(state): State<AppState>,
    Query(query): Query<PutFileQuery>,
    body: Body,
) -> Response {
    let path = match scratch_file() {
        Ok(p) => p,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if let Err(e) = save_body(body, &path).await {
        let _ = tokio::fs::remove_file(&path).await;
        return api_error(StatusCode::BAD_REQUEST, format!("upload interrupted: {e}"));
    }
    let req = ControlRequest::PutFile {
        path: path.clone(),
        network_id: query.network,
        ph: query.ph,
        file_name: query.name,
    };
    let response = respond(&state, req, StatusCode::CREATED).await;
    let _ = tokio::fs::remove_file(&path).await;
    response
}

#[derive(Deserialize)]
struct FileQuery {
    network: String,
}

#[derive(Deserialize, Default)]
struct NetworkQuery {
    #[serde(default)]
    network: String,
}

#[instrument(skip_all)]
async fn list_files(State(state): State<AppState>, Query(query): Query<NetworkQuery>) -> Response {
    respond(
        &state,
        ControlRequest::ListFiles {
            network_id: query.network,
        },
        StatusCode::OK,
    )
    .await
}

#[instrument(skip_all)]
async fn storage(State(state): State<AppState>, Query(query): Query<NetworkQuery>) -> Response {
    respond(
        &state,
        ControlRequest::StorageInfo {
            network_id: query.network,
        },
        StatusCode::OK,
    )
    .await
}

fn attachment(name: &str) -> String {
    let encoded: String = name
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("attachment; filename*=UTF-8''{encoded}")
}

#[instrument(skip_all)]
async fn get_file(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    Query(query): Query<FileQuery>,
) -> Response {
    let output = match scratch_file() {
        Ok(p) => p,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let req = ControlRequest::GetFile {
        file_id,
        network_id: query.network,
        output: output.clone(),
    };
    let data = match call_daemon(&state, req).await {
        Ok(data) => data,
        Err(resp) => return *resp,
    };
    let file = tokio::fs::File::open(&output).await;
    let _ = tokio::fs::remove_file(&output).await;
    let (meta, file) = match (serde_json::from_value::<GetFileData>(data), file) {
        (Ok(meta), Ok(file)) => (meta, file),
        (Err(e), _) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        (_, Err(e)) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let stream = futures::stream::unfold(file, |mut file| async move {
        let mut buf = vec![0u8; 1 << 16];
        match file.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, std::io::Error>(buf), file))
            }
            Err(e) => Some((Err(e), file)),
        }
    });
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_LENGTH, meta.size_bytes.to_string()),
            (header::CONTENT_DISPOSITION, attachment(&meta.file_name)),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

#[instrument(skip_all)]
async fn delete_file(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    Query(query): Query<FileQuery>,
) -> Response {
    respond(
        &state,
        ControlRequest::DeleteFile {
            file_id,
            network_id: query.network,
        },
        StatusCode::OK,
    )
    .await
}

#[instrument(skip_all)]
async fn unshare_file(
    State(state): State<AppState>,
    Path((file_id, member)): Path<(String, String)>,
    Query(query): Query<FileQuery>,
) -> Response {
    respond(
        &state,
        ControlRequest::Unshare {
            network_id: query.network,
            file_id,
            member,
        },
        StatusCode::OK,
    )
    .await
}

#[derive(Deserialize)]
struct RelayConnectBody {
    relay_addr: String,
}

#[instrument(skip(state, body))]
async fn relay_connect(
    State(state): State<AppState>,
    Json(body): Json<RelayConnectBody>,
) -> Response {
    respond(
        &state,
        ControlRequest::ConnectRelay {
            relay_addr: body.relay_addr,
        },
        StatusCode::OK,
    )
    .await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bp_api=info".parse().unwrap()),
        )
        .with_target(false)
        .init();

    let args = Args::parse();

    let socket_path = config::socket_path()?;
    let state = AppState {
        socket_path: Arc::new(socket_path),
        user_key: Arc::new(daemon_user_key().map_err(|e| {
            anyhow::anyhow!("cannot read this device's identity ({e}): run `bp login` first")
        })?),
    };
    let (cert_path, key_path, fingerprint) = load_or_create_tls()?;

    let api = Router::new()
        .route("/ping", get(ping))
        .route("/status", get(status))
        .route("/peers", get(peers))
        .route("/services", post(hatch))
        .route("/services/{service_id}", delete(farewell))
        .route("/services/{service_id}/pause", post(pause_service))
        .route("/services/{service_id}/resume", post(resume_service))
        .route("/networks", post(network_create))
        .route("/networks/{network_id}/invites", post(network_invite))
        .route("/networks/join", post(network_join))
        .route("/networks/{network_id}/members", get(network_members))
        .route("/networks/{network_id}/expulsions", post(network_expel))
        .route("/networks/leave", post(network_leave))
        .route("/files", get(list_files).post(put_file))
        .route("/storage", get(storage))
        .route("/files/{file_id}", get(get_file).delete(delete_file))
        .route("/files/{file_id}/shares", post(share_file))
        .route("/files/{file_id}/shares/{member}", delete(unshare_file))
        .route("/relay/connect", post(relay_connect))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ))
        .with_state(state);
    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/app.css", get(app_css))
        .route("/sw.js", get(service_worker))
        .route("/manifest.webmanifest", get(manifest))
        .route("/icon.svg", get(icon))
        .merge(api);

    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid bind address: {e}"))?;

    tracing::info!("bp-api listening on https://{addr}");
    tracing::info!("TLS certificate SHA-256 fingerprint: {fingerprint}");
    tracing::info!("Open the dashboard with: bp dashboard --port {}", args.port);
    tracing::info!(
        "Proxying requests to daemon at {:?}",
        config::socket_path()?
    );

    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path).await?;
    axum_server::bind_rustls(addr, tls)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}
