use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use bp_core::{
    config,
    control::{
        client,
        protocol::{ControlRequest, ControlResponse, GetFileData},
    },
    service::ServiceType,
};
use futures::StreamExt;
use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::instrument;

#[derive(Clone)]
struct AppState {
    socket_path: Arc<PathBuf>,
    user_key: Option<Arc<String>>,
}

async fn require_session(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let in_url = (request.method() == axum::http::Method::GET
        && request.uri().path().starts_with("/files/"))
    .then(|| Query::<HashMap<String, String>>::try_from_uri(request.uri()).ok())
    .flatten()
    .and_then(|Query(query)| query.get("session").cloned());
    let presented = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Session "))
        .map(str::to_string)
        .or(in_url)
        .unwrap_or_default();
    let presented = presented.as_str();
    let Some(user_key) = &state.user_key else {
        return next.run(request).await;
    };
    match bp_core::session::verify(presented, user_key, chrono_now()) {
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
    tier: Option<bp_core::storage::StorageTier>,
    chunk: Option<u64>,
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
        tier: query.tier,
        chunk_bytes: query.chunk,
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

const OPEN_RANGE_BYTES: u64 = 8 << 20;

fn media_type(name: &str) -> &'static str {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}

fn requested_range(headers: &HeaderMap) -> Option<(u64, u64)> {
    let spec = headers
        .get(header::RANGE)?
        .to_str()
        .ok()?
        .strip_prefix("bytes=")?;
    let (from, to) = spec.split_once('-')?;
    let from: u64 = from.trim().parse().ok()?;
    let to = to.trim();
    let len = if to.is_empty() {
        OPEN_RANGE_BYTES
    } else {
        to.parse::<u64>().ok()?.checked_sub(from)? + 1
    };
    Some((from, len))
}

#[instrument(skip_all)]
async fn get_file(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    Query(query): Query<FileQuery>,
    headers: HeaderMap,
) -> Response {
    let output = match scratch_file() {
        Ok(p) => p,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let range = requested_range(&headers);
    let req = ControlRequest::GetFile {
        file_id,
        network_id: query.network,
        output: output.clone(),
        range,
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
    let mut response = (
        [
            (
                header::CONTENT_TYPE,
                media_type(&meta.file_name).to_string(),
            ),
            (header::CONTENT_LENGTH, meta.bytes.to_string()),
            (header::CONTENT_DISPOSITION, attachment(&meta.file_name)),
            (header::ACCEPT_RANGES, "bytes".to_string()),
        ],
        Body::from_stream(stream),
    )
        .into_response();
    if let Some((from, _)) = range {
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
        let range = format!(
            "bytes {from}-{}/{}",
            from + meta.bytes.saturating_sub(1),
            meta.size_bytes
        );
        if let Ok(value) = header::HeaderValue::from_str(&range) {
            response.headers_mut().insert(header::CONTENT_RANGE, value);
        }
    }
    response
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

pub fn router(socket_path: PathBuf, user_key: Option<String>) -> Router {
    let state = AppState {
        socket_path: Arc::new(socket_path),
        user_key: user_key.map(Arc::new),
    };
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
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/app.css", get(app_css))
        .route("/sw.js", get(service_worker))
        .route("/manifest.webmanifest", get(manifest))
        .route("/icon.svg", get(icon))
        .merge(api)
}
