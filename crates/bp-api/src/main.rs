//! # bp-api — BillPouch REST API gateway
//!
//! Thin HTTP adapter that proxies REST requests to the running `bp` daemon
//! over the Unix control socket.  The daemon must already be running
//! (`bp hatch <type>`) before requests can be served.
//!
//! ## Endpoints
//!
//! | Method | Path                         | Description                        |
//! |--------|------------------------------|------------------------------------|
//! | GET    | `/ping`                      | Liveness check                     |
//! | GET    | `/status`                    | Daemon identity + services         |
//! | GET    | `/peers`                     | Known peers + network info         |
//! | POST   | `/services`                  | Hatch a new service                |
//! | DELETE | `/services/:service_id`      | Stop a service                     |
//! | POST   | `/networks/join`             | Join a gossip network              |
//! | POST   | `/networks/leave`            | Leave a gossip network             |
//! | POST   | `/files`                     | Store a file (base64 chunk_data)   |
//! | GET    | `/files/:chunk_id`           | Retrieve a file (`?network=public`)|//! | POST   | `/relay/connect`             | Dial a relay node for NAT traversal|//!
//! ## Usage
//!
//! ```text
//! bp-api --port 3000 --host 127.0.0.1
//! ```

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use bp_core::{
    config,
    control::protocol::{ControlRequest, ControlResponse},
    service::ServiceType,
};
use clap::Parser;
use serde::Deserialize;
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};
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
    /// TCP port to listen on.
    #[arg(long, default_value_t = 3000)]
    port: u16,

    /// Host address to bind.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

// ── App state ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    socket_path: Arc<PathBuf>,
}

// ── Daemon client ─────────────────────────────────────────────────────────────

/// Minimal async client for the bp daemon control socket.
struct DaemonClient {
    stream: UnixStream,
}

impl DaemonClient {
    async fn connect(path: &std::path::Path) -> anyhow::Result<Self> {
        let stream = UnixStream::connect(path).await?;
        Ok(Self { stream })
    }

    async fn call(&mut self, req: ControlRequest) -> anyhow::Result<serde_json::Value> {
        let (read_half, mut write_half) = self.stream.split();
        let mut json = serde_json::to_string(&req)?;
        json.push('\n');
        write_half.write_all(json.as_bytes()).await?;

        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let resp: ControlResponse = serde_json::from_str(line.trim())?;

        match resp {
            ControlResponse::Ok { data } => Ok(data.unwrap_or(serde_json::Value::Null)),
            ControlResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
        }
    }
}

// ── Helper macros ─────────────────────────────────────────────────────────────

/// Connect to the daemon, call `$req`, and return a JSON or error response.
macro_rules! daemon_call {
    ($state:expr, $req:expr) => {{
        let path = $state.socket_path.as_ref();
        match DaemonClient::connect(path).await {
            Err(e) => daemon_unavailable(e),
            Ok(mut client) => match client.call($req).await {
                Ok(data) => (StatusCode::OK, Json(data)).into_response(),
                Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            },
        }
    }};
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

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /ping` — liveness check.
#[instrument(skip_all)]
async fn ping(State(state): State<AppState>) -> Response {
    daemon_call!(state, ControlRequest::Ping)
}

/// `GET /status` — daemon identity and local services.
#[instrument(skip_all)]
async fn status(State(state): State<AppState>) -> Response {
    daemon_call!(state, ControlRequest::Status)
}

/// `GET /peers` — all known gossip peers.
#[instrument(skip_all)]
async fn peers(State(state): State<AppState>) -> Response {
    daemon_call!(state, ControlRequest::Flock)
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
    let path = state.socket_path.as_ref();
    match DaemonClient::connect(path).await {
        Err(e) => daemon_unavailable(e),
        Ok(mut client) => match client.call(req).await {
            Ok(data) => (StatusCode::CREATED, Json(data)).into_response(),
            Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        },
    }
}

/// `DELETE /services/:service_id` — stop a service.
#[instrument(skip_all)]
async fn farewell(State(state): State<AppState>, Path(service_id): Path<String>) -> Response {
    daemon_call!(state, ControlRequest::Farewell { service_id })
}

// ── /networks ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct NetworkBody {
    network_id: String,
}

/// `POST /networks/join` — subscribe to a gossip network.
#[instrument(skip_all)]
async fn network_join(State(state): State<AppState>, Json(body): Json<NetworkBody>) -> Response {
    daemon_call!(
        state,
        ControlRequest::Join {
            network_id: body.network_id
        }
    )
}

/// `POST /networks/leave` — unsubscribe from a gossip network.
#[instrument(skip_all)]
async fn network_leave(State(state): State<AppState>, Json(body): Json<NetworkBody>) -> Response {
    daemon_call!(
        state,
        ControlRequest::Leave {
            network_id: body.network_id
        }
    )
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
    };

    let path = state.socket_path.as_ref();
    match DaemonClient::connect(path).await {
        Err(e) => daemon_unavailable(e),
        Ok(mut client) => match client.call(req).await {
            Ok(data) => (StatusCode::CREATED, Json(data)).into_response(),
            Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        },
    }
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

    let path = state.socket_path.as_ref();
    match DaemonClient::connect(path).await {
        Err(e) => daemon_unavailable(e),
        Ok(mut client) => match client.call(req).await {
            Ok(data) => {
                // The chunk_data in the response is a JSON array of u8 from
                // serde; re-encode it as base64 for a clean HTTP response.
                if let Some(arr) = data.get("chunk_data") {
                    if let Ok(bytes) = serde_json::from_value::<Vec<u8>>(arr.clone()) {
                        return (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "chunk_id": chunk_id,
                                "data": B64.encode(&bytes),
                            })),
                        )
                            .into_response();
                    }
                }
                (StatusCode::OK, Json(data)).into_response()
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("not found") || msg.contains("NotFound") {
                    api_error(
                        StatusCode::NOT_FOUND,
                        format!("Chunk '{chunk_id}' not found"),
                    )
                } else {
                    api_error(StatusCode::INTERNAL_SERVER_ERROR, msg)
                }
            }
        },
    }
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
async fn relay_connect(State(state): State<AppState>, Json(body): Json<RelayConnectBody>) -> Response {
    daemon_call!(
        state,
        ControlRequest::ConnectRelay {
            relay_addr: body.relay_addr,
        }
    )
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
        .route("/ping", get(ping))
        .route("/status", get(status))
        .route("/peers", get(peers))
        .route("/services", post(hatch))
        .route("/services/:service_id", delete(farewell))
        .route("/networks/join", post(network_join))
        .route("/networks/leave", post(network_leave))
        .route("/files", post(put_file))
        .route("/files/:chunk_id", get(get_file))
        .route("/relay/connect", post(relay_connect))
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
