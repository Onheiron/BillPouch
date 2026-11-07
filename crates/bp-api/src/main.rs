//! # bp-api — BillPouch REST API gateway
//!
//! Thin HTTP adapter that forwards requests to the running daemon over the
//! Unix control socket, plus an embedded dashboard at `GET /`.
//! Endpoints: see `docs/api.md`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use bp_core::{
    config,
    control::{
        client,
        protocol::{ControlRequest, ControlResponse, GetFileData},
    },
    service::ServiceType,
};
use clap::Parser;
use serde::Deserialize;
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};
use tracing::instrument;

// ── CLI args ──────────────────────────────────────────────────────────────────

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

// ── App state ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    socket_path: Arc<PathBuf>,
}

// ── Daemon calls ──────────────────────────────────────────────────────────────

/// Send `req` to the daemon; `Ok(data)` on success, or a ready HTTP error
/// (503 if the daemon is unreachable, 500 if it returned an error).
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

/// Call the daemon and wrap its data as a JSON response with `status`.
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

// ── Dashboard (embedded) ─────────────────────────────────────────────────────

/// The single-page dashboard, embedded at compile time from `static/index.html`.
const DASHBOARD_HTML: &str = include_str!("../static/index.html");

/// `GET /` — serve the web dashboard.
async fn dashboard() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

// ── Liveness ──────────────────────────────────────────────────────────────────

/// `GET /ping` — liveness check.
#[instrument(skip_all)]
async fn ping(State(state): State<AppState>) -> Response {
    respond(&state, ControlRequest::Ping, StatusCode::OK).await
}

/// `GET /status` — daemon identity and local services.
#[instrument(skip_all)]
async fn status(State(state): State<AppState>) -> Response {
    respond(&state, ControlRequest::Status, StatusCode::OK).await
}

/// `GET /peers` — all known gossip peers.
#[instrument(skip_all)]
async fn peers(State(state): State<AppState>) -> Response {
    respond(&state, ControlRequest::Flock, StatusCode::OK).await
}

// ── /services ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct HatchBody {
    service_type: ServiceType,
    network_id: String,
    #[serde(default)]
    metadata: HashMap<String, serde_json::Value>,
}

/// `POST /services` — hatch (start) a new service.
#[instrument(skip_all)]
async fn hatch(State(state): State<AppState>, Json(body): Json<HatchBody>) -> Response {
    let req = ControlRequest::Hatch {
        service_type: body.service_type,
        network_id: body.network_id,
        metadata: body.metadata,
    };
    respond(&state, req, StatusCode::CREATED).await
}

/// Optional query parameters for `DELETE /services/:service_id`.
#[derive(Deserialize, Default)]
struct FarewellQuery {
    /// If `true`, the daemon also evicts all fragments stored by this service.
    #[serde(default)]
    evict: bool,
}

/// `DELETE /services/:service_id[?evict=true]` — stop (and optionally evict) a service.
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

// ── /services/:id/pause  /services/:id/resume ─────────────────────────────────

#[derive(Deserialize)]
struct PauseBody {
    eta_minutes: u64,
}

/// `POST /services/:service_id/pause` — pause a running service.
///
/// Body JSON: `{ "eta_minutes": 30 }`
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

/// `POST /services/:service_id/resume` — resume a paused service.
#[instrument(skip_all)]
async fn resume_service(State(state): State<AppState>, Path(service_id): Path<String>) -> Response {
    respond(
        &state,
        ControlRequest::Resume { service_id },
        StatusCode::OK,
    )
    .await
}

// ── /networks ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct NetworkBody {
    network_id: String,
    /// Auto-evict all active services before leaving (only used by `POST /networks/leave`).
    #[serde(default)]
    force: bool,
}

/// `POST /networks/join` — subscribe to a gossip network.
#[instrument(skip_all)]
async fn network_join(State(state): State<AppState>, Json(body): Json<NetworkBody>) -> Response {
    respond(
        &state,
        ControlRequest::Join {
            network_id: body.network_id,
        },
        StatusCode::OK,
    )
    .await
}

/// `POST /networks/leave` — unsubscribe from a gossip network.
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

// ── /files ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PutFileBody {
    /// Base64-encoded bytes of the file to store.
    chunk_data: String,
    /// Network whose Pouches will store the fragments.
    #[serde(default = "default_network")]
    network_id: String,
    /// Target recovery probability (default: 0.999).
    ph: Option<f64>,
    /// Redundancy overhead (default: 1.0).
    q_target: Option<f64>,
    /// Human-readable name for the file (shown by `bp ls`).
    file_name: Option<String>,
}

fn default_network() -> String {
    "public".into()
}

/// `POST /files` — encode and store a file chunk.
///
/// Body JSON: `{ "chunk_data": "<base64>", "network_id": "public", "ph": 0.999, "q_target": 1.0 }`
/// Returns: `{ "chunk_id": "<hex>" }`
#[instrument(skip_all)]
async fn put_file(State(state): State<AppState>, Json(body): Json<PutFileBody>) -> Response {
    let raw = match B64.decode(&body.chunk_data) {
        Ok(b) => b,
        Err(e) => {
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("chunk_data is not valid base64: {e}"),
            )
        }
    };

    let req = ControlRequest::PutFile {
        chunk_data: raw,
        network_id: body.network_id,
        ph: body.ph,
        q_target: body.q_target,
        file_name: body.file_name,
    };

    respond(&state, req, StatusCode::CREATED).await
}

#[derive(Deserialize)]
struct GetFileQuery {
    #[serde(default = "default_network")]
    network: String,
}

/// `GET /files/:chunk_id` — retrieve and decode a stored file.
///
/// Query: `?network=public` (optional, defaults to "public")
/// Returns: `{ "chunk_id": "...", "data": "<base64>" }` or HTTP 404.
#[instrument(skip_all)]
async fn get_file(
    State(state): State<AppState>,
    Path(chunk_id): Path<String>,
    Query(query): Query<GetFileQuery>,
) -> Response {
    let req = ControlRequest::GetFile {
        chunk_id: chunk_id.clone(),
        network_id: query.network,
    };

    let data = match call_daemon(&state, req).await {
        Ok(data) => data,
        Err(resp) => return *resp,
    };
    // `data` is serialized by serde as a JSON array of u8 — return base64.
    match serde_json::from_value::<GetFileData>(data) {
        Ok(d) => Json(serde_json::json!({
            "chunk_id": d.chunk_id,
            "data": B64.encode(&d.data),
            "fragments_used": d.fragments_used,
            "fragments_remote": d.fragments_remote,
        }))
        .into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

// ── /invites ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateInviteBody {
    network_id: String,
    invite_password: String,
    #[serde(default)]
    invitee_fingerprint: Option<String>,
    #[serde(default)]
    ttl_hours: Option<u64>,
}

/// `POST /invites` — create a signed invite token for a network.
///
/// Body JSON: `{ "network_id": "friends", "invite_password": "secret" }`
/// Returns: `{ "blob": "<hex>", "network_id": "...", ... }`
#[instrument(skip_all)]
async fn create_invite(
    State(state): State<AppState>,
    Json(body): Json<CreateInviteBody>,
) -> Response {
    respond(
        &state,
        ControlRequest::CreateInvite {
            network_id: body.network_id,
            invitee_fingerprint: body.invitee_fingerprint,
            invite_password: body.invite_password,
            ttl_hours: body.ttl_hours,
        },
        StatusCode::OK,
    )
    .await
}

#[derive(Deserialize)]
struct RedeemInviteBody {
    blob: String,
    invite_password: String,
}

/// `POST /invites/redeem` — redeem an invite token and join the network.
///
/// Body JSON: `{ "blob": "<hex>", "invite_password": "secret" }`
///
/// The crypto (decrypt + verify) runs locally inside bp-api.  After saving
/// the NetworkMetaKey the daemon is asked to subscribe to the network topics.
#[instrument(skip_all)]
async fn redeem_invite(
    State(state): State<AppState>,
    Json(body): Json<RedeemInviteBody>,
) -> Response {
    // 1. Decrypt and verify the invite token locally.
    let payload = match bp_core::invite::redeem_invite(&body.blob, &body.invite_password) {
        Ok(p) => p,
        Err(e) => return api_error(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
    };

    // 2. Persist the NetworkMetaKey.
    if let Err(e) = bp_core::invite::save_invite_key(&payload) {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }

    let network_id = payload.network_id.clone();

    // 3. Ask the daemon to join (subscribe gossip topics).
    // Ignore join errors (e.g. already joined) — the key is already saved.
    let _ = call_daemon(
        &state,
        ControlRequest::Join {
            network_id: network_id.clone(),
        },
    )
    .await;
    Json(serde_json::json!({
        "status": "joined",
        "network_id": network_id,
        "inviter_fingerprint": payload.inviter_fingerprint,
    }))
    .into_response()
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Body for `POST /relay/connect`.
#[derive(Deserialize)]
struct RelayConnectBody {
    /// Full multiaddr of the relay node, e.g.
    /// `/ip4/1.2.3.4/tcp/4001/p2p/12D3KooW...`
    relay_addr: String,
}

/// `POST /relay/connect` — dial a relay node for NAT traversal.
///
/// Body: `{ "relay_addr": "/ip4/..." }`
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
    };

    let app = Router::new()
        .route("/", get(dashboard))
        .route("/ping", get(ping))
        .route("/status", get(status))
        .route("/peers", get(peers))
        .route("/services", post(hatch))
        .route("/services/:service_id", delete(farewell))
        .route("/services/:service_id/pause", post(pause_service))
        .route("/services/:service_id/resume", post(resume_service))
        .route("/networks/join", post(network_join))
        .route("/networks/leave", post(network_leave))
        .route("/files", post(put_file))
        .route("/files/:chunk_id", get(get_file))
        .route("/relay/connect", post(relay_connect))
        .route("/invites", post(create_invite))
        .route("/invites/redeem", post(redeem_invite))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid bind address: {e}"))?;

    tracing::info!("bp-api listening on http://{addr}");
    tracing::info!(
        "Proxying requests to daemon at {:?}",
        config::socket_path()?
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
