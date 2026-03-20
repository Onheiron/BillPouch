//! Unix socket control server — runs inside the daemon.
//! Each CLI invocation connects, sends one JSON request, reads one JSON response,
//! then closes the connection.

use crate::{
    coding::{params as coding_params, rlnc},
    control::protocol::{
        ControlRequest, ControlResponse, FlockData, GetFileData, HatchData, PutFileData, StatusData,
    },
    error::BpResult,
    identity::Identity,
    network::{state::NodeInfo, FragmentResponse, NetworkCommand, QosRegistry, StorageManagerMap},
    service::{ServiceInfo, ServiceRegistry, ServiceStatus, ServiceType},
    storage::StorageManager,
};
use libp2p::PeerId;
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
    /// One `StorageManager` per active Pouch service, keyed by `service_id`.
    ///
    /// Shared with the network loop so incoming fragment-fetch requests can
    /// be served directly from the P2P event handler.
    pub storage_managers: StorageManagerMap,
    /// Per-peer QoS data updated by the network quality monitor.
    /// Used by `PutFile` to derive adaptive k/n from live stability scores.
    pub qos: Arc<RwLock<QosRegistry>>,
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

    let listener =
        UnixListener::bind(path).map_err(|e| crate::error::BpError::Control(e.to_string()))?;

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
        ControlRequest::Hatch {
            service_type,
            network_id,
            metadata,
        } => {
            // Ensure we're subscribed to that network's gossip
            let already_joined = {
                let nets = state.networks.read().unwrap();
                nets.contains(&network_id)
            };
            if !already_joined {
                let _ = state
                    .net_tx
                    .send(NetworkCommand::JoinNetwork {
                        network_id: network_id.clone(),
                    })
                    .await;
                state.networks.write().unwrap().push(network_id.clone());
            }

            let info = ServiceInfo::new(service_type, network_id.clone(), metadata.clone());
            let service_id = info.id.clone();

            // Set to Running immediately — scope the lock guard so it is dropped
            // BEFORE the .await below (std::sync guards are !Send across await).
            {
                let mut reg = state.services.write().unwrap();
                let mut running_info = info.clone();
                running_info.status = ServiceStatus::Running;
                reg.register(running_info);
            } // ← lock released here

            // Initialise a StorageManager when hatching a Pouch service.
            if service_type == ServiceType::Pouch {
                let quota = metadata
                    .get("storage_bytes")
                    .or_else(|| metadata.get("storage_bytes_bid"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                match StorageManager::init(network_id.clone(), service_id.clone(), quota) {
                    Ok(sm) => {
                        state
                            .storage_managers
                            .write()
                            .unwrap()
                            .insert(service_id.clone(), Arc::new(RwLock::new(sm)));
                        tracing::info!(service_id=%service_id, quota_bytes=%quota, "StorageManager initialised");
                    }
                    Err(e) => {
                        tracing::warn!("Failed to init StorageManager for {}: {}", service_id, e);
                    }
                }
            }

            // Announce to the network (await-safe: no lock held)
            announce_self(state, &service_id, service_type, &network_id).await;

            ControlResponse::ok(HatchData {
                service_id: service_id.clone(),
                service_type,
                network_id,
                message: format!("🦤 {} service hatched — id: {}", service_type, service_id),
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
                None => ControlResponse::err(format!("No service with id '{}'", service_id)),
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
                        "Already a member of network '{}'",
                        network_id
                    ));
                }
            }
            let _ = state
                .net_tx
                .send(NetworkCommand::JoinNetwork {
                    network_id: network_id.clone(),
                })
                .await;
            state.networks.write().unwrap().push(network_id.clone());
            ControlResponse::ok(serde_json::json!({
                "network_id": network_id,
                "message": format!("Joined network '{}'", network_id),
            }))
        }

        // ── Leave ─────────────────────────────────────────────────────────
        ControlRequest::Leave { network_id } => {
            let _ = state
                .net_tx
                .send(NetworkCommand::LeaveNetwork {
                    network_id: network_id.clone(),
                })
                .await;
            state.networks.write().unwrap().retain(|n| n != &network_id);
            ControlResponse::ok(serde_json::json!({
                "network_id": network_id,
                "message": format!("Left network '{}'", network_id),
            }))
        }

        // ── PutFile ───────────────────────────────────────────────────────
        ControlRequest::PutFile {
            chunk_data,
            ph,
            q_target,
            network_id,
        } => {
            let ph = ph.unwrap_or(0.999);
            let q_target = q_target.unwrap_or(1.0);

            // Find a Pouch StorageManager for this network.
            let managers_snap: Vec<(String, Arc<RwLock<StorageManager>>)> = {
                let map = state.storage_managers.read().unwrap();
                map.iter()
                    .map(|(id, sm)| (id.clone(), Arc::clone(sm)))
                    .collect()
            };

            // Filter to managers whose network matches (or any if network_id is empty).
            let candidates: Vec<_> = managers_snap
                .iter()
                .filter(|(_, sm)| {
                    let meta = &sm.read().unwrap().meta;
                    network_id.is_empty() || meta.network_id == network_id
                })
                .collect();

            if candidates.is_empty() {
                return ControlResponse::err(format!(
                    "No active Pouch on network '{}' — run `bp hatch pouch --network {0}` first",
                    network_id
                ));
            }

            // ── Compute adaptive k/n from live QoS data ──────────────────────
            let pouch_peer_ids: Vec<String> = {
                let ns = state.network_state.read().unwrap();
                ns.in_network(&network_id)
                    .into_iter()
                    .filter(|n| n.service_type == ServiceType::Pouch)
                    .map(|n| n.peer_id.clone())
                    .collect()
            };

            // For peers not yet observed via Ping, assume 0.8 stability
            // (benefit of the doubt for new entries).
            let stabilities: Vec<f64> = {
                let qos = state.qos.read().unwrap();
                if pouch_peer_ids.is_empty() {
                    vec![0.8; candidates.len().max(1)]
                } else {
                    pouch_peer_ids
                        .iter()
                        .map(|pid| match qos.get(pid) {
                            Some(p) if p.is_observed() => p.stability_score(),
                            _ => 0.8,
                        })
                        .collect()
                }
            };

            let (k, n, q, pe) =
                match coding_params::compute_coding_params(&stabilities, ph, q_target) {
                    Ok(p) => {
                        let pe = coding_params::effective_recovery_probability(&stabilities, p.k);
                        (p.k, p.n, p.q, pe)
                    }
                    Err(e) => {
                        tracing::warn!("compute_coding_params failed ({e}), using fallback");
                        let peer_n = stabilities.len().max(2);
                        let fallback_k = (peer_n / 2).max(1);
                        let fallback_q = (peer_n - fallback_k) as f64 / fallback_k as f64;
                        let pe =
                            coding_params::effective_recovery_probability(&stabilities, fallback_k);
                        (fallback_k, peer_n, fallback_q, pe)
                    }
                };

            tracing::info!(
                network = %network_id, k = %k, n = %n, q = %q,
                ph = %ph, pe = %pe, peers = %stabilities.len(),
                "PutFile: computed coding params"
            );

            // Encode the chunk.
            let fragments = match rlnc::encode(&chunk_data, k, n) {
                Ok(f) => f,
                Err(e) => return ControlResponse::err(format!("Encode error: {e}")),
            };

            let chunk_id = match fragments.first() {
                Some(f) => f.chunk_id.clone(),
                None => return ControlResponse::err("Encoding produced no fragments"),
            };

            // Store all fragments locally in the first candidate with capacity.
            let mut stored = 0usize;
            {
                let (_, sm_arc) = candidates[0];
                let mut sm = sm_arc.write().unwrap();
                for fragment in &fragments {
                    match sm.store_fragment(fragment) {
                        Ok(()) => stored += 1,
                        Err(e) => {
                            tracing::warn!(chunk_id=%chunk_id, "store_fragment failed: {e}");
                            break;
                        }
                    }
                }
            } // write lock released before async work

            if stored == 0 {
                return ControlResponse::err("Failed to store any fragments (quota exceeded?)");
            }

            // ── Distribute fragments to remote Pouch peers (round-robin) ────
            let remote_pouches: Vec<PeerId> = {
                let ns = state.network_state.read().unwrap();
                let local_peer = state.identity.peer_id.to_string();
                ns.in_network(&network_id)
                    .into_iter()
                    .filter(|n| n.service_type == ServiceType::Pouch && n.peer_id != local_peer)
                    .filter_map(|n| n.peer_id.parse::<PeerId>().ok())
                    .collect()
            };

            let mut distributed = 0usize;
            if !remote_pouches.is_empty() {
                for (i, fragment) in fragments.iter().enumerate() {
                    let peer = remote_pouches[i % remote_pouches.len()];
                    if state
                        .net_tx
                        .send(NetworkCommand::PushFragment {
                            peer_id: peer,
                            chunk_id: chunk_id.clone(),
                            fragment_id: fragment.id.clone(),
                            data: fragment.to_bytes(),
                        })
                        .await
                        .is_ok()
                    {
                        distributed += 1;
                    }
                }
            }

            tracing::info!(
                chunk_id=%chunk_id, stored=%stored, distributed=%distributed,
                total=%fragments.len(), "PutFile stored + distributed"
            );

            ControlResponse::ok(PutFileData {
                chunk_id: chunk_id.clone(),
                k,
                n,
                q,
                ph,
                pe,
                fragments_stored: stored,
                fragments_distributed: distributed,
                message: format!(
                    "Stored {stored}/{total} locally, pushed {distributed} to {peers} remote pouch(es) — chunk_id: {chunk_id}",
                    total = fragments.len(),
                    peers = remote_pouches.len(),
                ),
            })
        }

        // ── GetFile ───────────────────────────────────────────────────────
        ControlRequest::GetFile {
            chunk_id,
            network_id,
        } => {
            let managers_snap: Vec<Arc<RwLock<StorageManager>>> = {
                let map = state.storage_managers.read().unwrap();
                map.values()
                    .filter(|sm| {
                        let meta = &sm.read().unwrap().meta;
                        network_id.is_empty() || meta.network_id == network_id
                    })
                    .map(Arc::clone)
                    .collect()
            };

            if managers_snap.is_empty() {
                return ControlResponse::err("No active Pouch found — daemon has no storage");
            }

            // Collect fragments from all matching local managers.
            let mut all_fragments = Vec::new();
            for sm_arc in &managers_snap {
                let sm = sm_arc.read().unwrap();
                let metas = sm.index.fragments_for_chunk(&chunk_id).to_vec();
                for meta in metas {
                    match sm.load_fragment(&chunk_id, &meta.fragment_id) {
                        Ok(f) => all_fragments.push(f),
                        Err(e) => tracing::warn!("load_fragment failed: {e}"),
                    }
                }
            }

            let local_count = all_fragments.len();

            // Determine k from existing fragments (if any) to know if we need more.
            let k_needed = all_fragments.first().map(|f| f.k).unwrap_or(0);

            // ── Fetch from remote Pouches if local fragments < k ────────────
            let mut fragments_remote = 0usize;
            if k_needed > 0 && all_fragments.len() < k_needed {
                let remote_pouches: Vec<PeerId> = {
                    let ns = state.network_state.read().unwrap();
                    let local_peer = state.identity.peer_id.to_string();
                    ns.in_network(&network_id)
                        .into_iter()
                        .filter(|n| n.service_type == ServiceType::Pouch && n.peer_id != local_peer)
                        .filter_map(|n| n.peer_id.parse::<PeerId>().ok())
                        .collect()
                };

                // Send FetchChunkFragments to each remote Pouch.
                let mut receivers = Vec::new();
                for peer in &remote_pouches {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    if state
                        .net_tx
                        .send(NetworkCommand::FetchChunkFragments {
                            peer_id: *peer,
                            chunk_id: chunk_id.clone(),
                            resp_tx: tx,
                        })
                        .await
                        .is_ok()
                    {
                        receivers.push(rx);
                    }
                }

                // Await responses with a timeout.
                use crate::coding::rlnc::EncodedFragment;
                for rx in receivers {
                    match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
                        Ok(Ok(FragmentResponse::FoundMany { fragments })) => {
                            for (frag_id, bytes) in fragments {
                                if all_fragments.len() >= k_needed {
                                    break;
                                }
                                match EncodedFragment::from_bytes(frag_id, chunk_id.clone(), &bytes)
                                {
                                    Ok(f) => {
                                        all_fragments.push(f);
                                        fragments_remote += 1;
                                    }
                                    Err(e) => tracing::warn!("Remote fragment parse error: {e}"),
                                }
                            }
                        }
                        Ok(Ok(FragmentResponse::Found { data })) => {
                            if all_fragments.len() < k_needed {
                                let frag_id = uuid::Uuid::new_v4().to_string();
                                match EncodedFragment::from_bytes(frag_id, chunk_id.clone(), &data)
                                {
                                    Ok(f) => {
                                        all_fragments.push(f);
                                        fragments_remote += 1;
                                    }
                                    Err(e) => tracing::warn!("Remote fragment parse error: {e}"),
                                }
                            }
                        }
                        Ok(Ok(_)) => {} // NotFound, Stored, StoreFailed — skip
                        Ok(Err(_)) => tracing::debug!("Remote fetch: oneshot dropped"),
                        Err(_) => tracing::debug!("Remote fetch: timeout"),
                    }
                    if all_fragments.len() >= k_needed {
                        break;
                    }
                }
            }

            if all_fragments.is_empty() {
                return ControlResponse::err(format!(
                    "Chunk '{chunk_id}' not found locally or on remote peers"
                ));
            }

            let fragments_used = all_fragments.len();
            match rlnc::decode(&all_fragments) {
                Ok(data) => {
                    tracing::info!(
                        chunk_id=%chunk_id, local=%local_count,
                        remote=%fragments_remote, total=%fragments_used,
                        "GetFile decoded"
                    );
                    ControlResponse::ok(GetFileData {
                        chunk_id,
                        data,
                        fragments_used,
                        fragments_remote,
                    })
                }
                Err(e) => ControlResponse::err(format!("Decode failed: {e}")),
            }
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
            let _ = state
                .net_tx
                .send(NetworkCommand::Announce {
                    network_id: network_id.to_string(),
                    payload,
                })
                .await;
        }
        Err(e) => tracing::warn!("Failed to serialize NodeInfo: {}", e),
    }
}
