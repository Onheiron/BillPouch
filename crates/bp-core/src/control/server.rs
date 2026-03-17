//! Unix socket control server — runs inside the daemon.
//! Each CLI invocation connects, sends one JSON request, reads one JSON response,
//! then closes the connection.

use crate::{
    control::protocol::{
        ControlRequest, ControlResponse, FlockData, HatchData, StatusData,
    },
    error::BpResult,
    network::{state::NodeInfo, NetworkCommand},
    service::{ServiceInfo, ServiceRegistry, ServiceStatus, ServiceType},
    identity::Identity,
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, RwLock},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::mpsc,
};

/// Shared daemon state accessible from the control server.
pub struct DaemonState {
    pub identity: Identity,
    pub services: RwLock<ServiceRegistry>,
    pub network_state: Arc<RwLock<crate::network::state::NetworkState>>,
    pub networks: RwLock<Vec<String>>,
    pub net_tx: mpsc::Sender<NetworkCommand>,
}

/// Accept connections on the Unix socket and dispatch requests.
pub async fn run_control_server(
    socket_path: impl AsRef<Path>,
    state: Arc<DaemonState>,
) -> BpResult<()> {
    // Remove stale socket from a previous run.
    let path = socket_path.as_ref();
    if path.exists() {
        std::fs::remove_file(path).ok();
    }

    let listener = UnixListener::bind(path)
        .map_err(|e| crate::error::BpError::Control(e.to_string()))?;

    tracing::info!("Control socket listening at {:?}", path);

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, state).await {
                        tracing::warn!("Control connection error: {}", e);
                    }
                });
            }
            Err(e) => {
                tracing::error!("Accept error on control socket: {}", e);
            }
        }
    }
}

async fn handle_connection(stream: UnixStream, state: Arc<DaemonState>) -> anyhow::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    reader.read_line(&mut line).await?;
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    let request: ControlRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            let resp = ControlResponse::err(format!("Invalid request JSON: {}", e));
            send_response(&mut write_half, &resp).await?;
            return Ok(());
        }
    };

    let response = dispatch(request, &state).await;
    send_response(&mut write_half, &response).await?;
    Ok(())
}

async fn send_response(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    resp: &ControlResponse,
) -> anyhow::Result<()> {
    let mut json = serde_json::to_string(resp)?;
    json.push('\n');
    writer.write_all(json.as_bytes()).await?;
    Ok(())
}

/// Dispatch a `ControlRequest` to the appropriate handler.
async fn dispatch(req: ControlRequest, state: &Arc<DaemonState>) -> ControlResponse {
    match req {
        // ── Ping ──────────────────────────────────────────────────────────
        ControlRequest::Ping => ControlResponse::ok("pong"),

        // ── Status ────────────────────────────────────────────────────────
        ControlRequest::Status => {
            let services = state.services.read().unwrap();
            let networks = state.networks.read().unwrap();
            let ns = state.network_state.read().unwrap();
            let data = StatusData {
                peer_id: state.identity.peer_id.to_string(),
                fingerprint: state.identity.fingerprint.clone(),
                alias: state.identity.profile.alias.clone(),
                local_services: services.all().into_iter().cloned().collect(),
                networks: networks.clone(),
                known_peers: ns.len(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            };
            ControlResponse::ok(data)
        }

        // ── Hatch ─────────────────────────────────────────────────────────
        ControlRequest::Hatch { service_type, network_id, metadata } => {
            // Ensure we're subscribed to that network's gossip
            let already_joined = {
                let nets = state.networks.read().unwrap();
                nets.contains(&network_id)
            };
            if !already_joined {
                let _ = state.net_tx.send(NetworkCommand::JoinNetwork {
                    network_id: network_id.clone(),
                }).await;
                state.networks.write().unwrap().push(network_id.clone());
            }

            let info = ServiceInfo::new(service_type, network_id.clone(), metadata);
            let service_id = info.id.clone();

            // Set to Running immediately — scope the lock guard so it is dropped
            // BEFORE the .await below (std::sync guards are !Send across await).
            {
                let mut reg = state.services.write().unwrap();
                let mut running_info = info.clone();
                running_info.status = ServiceStatus::Running;
                reg.register(running_info);
            } // ← lock released here

            // Announce to the network (await-safe: no lock held)
            announce_self(state, &service_id, service_type, &network_id).await;

            ControlResponse::ok(HatchData {
                service_id: service_id.clone(),
                service_type,
                network_id,
                message: format!(
                    "🦤 {} service hatched — id: {}",
                    service_type, service_id
                ),
            })
        }

        // ── Farewell ──────────────────────────────────────────────────────
        ControlRequest::Farewell { service_id } => {
            let mut reg = state.services.write().unwrap();
            match reg.remove(&service_id) {
                Some(info) => ControlResponse::ok(serde_json::json!({
                    "service_id": info.id,
                    "service_type": info.service_type,
                    "message": format!("Service {} stopped", info.id),
                })),
                None => ControlResponse::err(format!(
                    "No service with id '{}'", service_id
                )),
            }
        }

        // ── Flock ─────────────────────────────────────────────────────────
        ControlRequest::Flock => {
            let services = state.services.read().unwrap();
            let networks = state.networks.read().unwrap();
            let ns = state.network_state.read().unwrap();
            let data = FlockData {
                local_services: services.all().into_iter().cloned().collect(),
                known_peers: ns.all().into_iter().cloned().collect(),
                networks: networks.clone(),
                peer_count: ns.len(),
            };
            ControlResponse::ok(data)
        }

        // ── Join ──────────────────────────────────────────────────────────
        ControlRequest::Join { network_id } => {
            {
                let nets = state.networks.read().unwrap();
                if nets.contains(&network_id) {
                    return ControlResponse::err(format!(
                        "Already a member of network '{}'", network_id
                    ));
                }
            }
            let _ = state.net_tx.send(NetworkCommand::JoinNetwork {
                network_id: network_id.clone(),
            }).await;
            state.networks.write().unwrap().push(network_id.clone());
            ControlResponse::ok(serde_json::json!({
                "network_id": network_id,
                "message": format!("Joined network '{}'", network_id),
            }))
        }

        // ── Leave ─────────────────────────────────────────────────────────
        ControlRequest::Leave { network_id } => {
            let _ = state.net_tx.send(NetworkCommand::LeaveNetwork {
                network_id: network_id.clone(),
            }).await;
            state.networks.write().unwrap().retain(|n| n != &network_id);
            ControlResponse::ok(serde_json::json!({
                "network_id": network_id,
                "message": format!("Left network '{}'", network_id),
            }))
        }
    }
}

/// Build a `NodeInfo` from the current daemon state for a given service and
/// broadcast it via gossipsub.
async fn announce_self(
    state: &Arc<DaemonState>,
    service_id: &str,
    service_type: ServiceType,
    network_id: &str,
) {
    let listen_addrs: Vec<String> = vec![]; // Populated once we know our Multiaddrs

    let info = NodeInfo {
        peer_id: state.identity.peer_id.to_string(),
        user_fingerprint: state.identity.fingerprint.clone(),
        user_alias: state.identity.profile.alias.clone(),
        service_type,
        service_id: service_id.to_string(),
        network_id: network_id.to_string(),
        listen_addrs,
        announced_at: chrono::Utc::now().timestamp() as u64,
        metadata: HashMap::new(),
    };

    match serde_json::to_vec(&info) {
        Ok(payload) => {
            let _ = state.net_tx.send(NetworkCommand::Announce {
                network_id: network_id.to_string(),
                payload,
            }).await;
        }
        Err(e) => tracing::warn!("Failed to serialize NodeInfo: {}", e),
    }
}
