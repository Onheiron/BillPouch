use crate::{
    coding::{params as coding_params, rlnc},
    control::protocol::{
        ControlRequest, ControlResponse, FileEntry, FlockData, GetFileData, HatchData, InviteData,
        ListFilesData, NetworkQosSummary, PouchStat, PutFileData, StatusData, StorageInfoData,
    },
    error::BpResult,
    identity::Identity,
    membership::{Memberships, Registry},
    network::{
        fragment_gossip::{FragmentIndexAnnouncement, FragmentPointer, RemoteFragmentIndex},
        state::NodeInfo,
        FragmentResponse, Gossip, NetworkCommand, OutgoingAssignments, QosRegistry,
        ReputationStore, StorageManagerMap,
    },
    service::{ServiceInfo, ServiceRegistry, ServiceStatus, ServiceType},
    storage::{ChunkCipher, FileRegistry, StorageManager, StorageTier},
};
use libp2p::{Multiaddr, PeerId};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::mpsc,
};

pub fn load_cek_hints() -> HashMap<String, [u8; 32]> {
    let path = match crate::config::cek_hints_path() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("cek_hints_path error: {e}");
            return HashMap::new();
        }
    };
    load_cek_hints_at(&path)
}

fn load_cek_hints_at(path: &std::path::Path) -> HashMap<String, [u8; 32]> {
    if !path.exists() {
        return HashMap::new();
    }
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Failed to read cek_hints: {e}");
            return HashMap::new();
        }
    };
    let hex_map: HashMap<String, String> = match serde_json::from_str(&data) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("Failed to parse cek_hints: {e}");
            return HashMap::new();
        }
    };
    hex_map
        .into_iter()
        .filter_map(|(k, v)| {
            let bytes = hex::decode(&v).ok()?;
            if bytes.len() != 32 {
                return None;
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            Some((k, arr))
        })
        .collect()
}

fn persist_cek_hints(hints: &HashMap<String, [u8; 32]>) {
    let path = match crate::config::cek_hints_path() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("cek_hints_path error: {e}");
            return;
        }
    };
    persist_cek_hints_at(&path, hints);
}

fn persist_cek_hints_at(path: &std::path::Path, hints: &HashMap<String, [u8; 32]>) {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("Failed to create cek_hints dir: {e}");
            return;
        }
    }
    let hex_map: HashMap<&str, String> = hints
        .iter()
        .map(|(k, v)| (k.as_str(), hex::encode(v)))
        .collect();
    match serde_json::to_string(&hex_map) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                tracing::warn!("Failed to write cek_hints: {e}");
            }
        }
        Err(e) => tracing::warn!("Failed to serialize cek_hints: {e}"),
    }
}

pub struct DaemonState {
    pub identity: Identity,
    pub services: RwLock<ServiceRegistry>,
    pub network_state: Arc<RwLock<crate::network::state::NetworkState>>,
    pub networks: RwLock<Vec<String>>,
    pub memberships: Memberships,
    pub net_tx: mpsc::Sender<NetworkCommand>,
    pub storage_managers: StorageManagerMap,
    pub qos: Arc<RwLock<QosRegistry>>,
    pub outgoing_assignments: OutgoingAssignments,
    pub remote_fragment_index: Arc<RwLock<RemoteFragmentIndex>>,
    pub chunk_cek_hints: RwLock<HashMap<String, [u8; 32]>>,
    pub reputation: Arc<RwLock<ReputationStore>>,
    pub file_registry: RwLock<FileRegistry>,
}

pub async fn run_control_server(
    socket_path: impl AsRef<Path>,
    state: Arc<DaemonState>,
) -> BpResult<()> {
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

async fn dispatch(req: ControlRequest, state: &Arc<DaemonState>) -> ControlResponse {
    match req {
        ControlRequest::Ping => ControlResponse::ok("pong"),

        ControlRequest::Status => {
            let services = state.services.read().unwrap();
            let networks = state.networks.read().unwrap();
            let ns = state.network_state.read().unwrap();
            let own_peer = state.identity.peer_id.to_string();

            let (rep_tier, rep_score) = {
                let rep = state.reputation.read().unwrap();
                rep.get(&own_peer)
                    .map_or((crate::network::ReputationTier::R1.to_string(), 0), |r| {
                        (r.tier.to_string(), r.reputation_score)
                    })
            };
            let pouch_stats = pouch_stats(state, &services, "");

            let qos_peer_count = state.qos.read().unwrap().peer_count();
            let network_qos = {
                let qos = state.qos.read().unwrap();
                let rep = state.reputation.read().unwrap();
                let peer_count = qos.peer_count();
                if peer_count == 0 {
                    None
                } else {
                    let scores = qos.all_stability_scores();
                    let avg_stability = if scores.is_empty() {
                        0.0
                    } else {
                        scores.iter().sum::<f64>() / scores.len() as f64
                    };
                    let mut tier_counts: std::collections::HashMap<String, usize> =
                        std::collections::HashMap::new();
                    for peer_id in qos.peer_ids() {
                        let tier = rep.tier(peer_id).to_string();
                        *tier_counts.entry(tier).or_insert(0) += 1;
                    }
                    Some(NetworkQosSummary {
                        observed_peers: peer_count,
                        avg_stability,
                        tier_counts,
                    })
                }
            };

            let data = StatusData {
                peer_id: own_peer,
                fingerprint: state.identity.fingerprint.clone(),
                alias: state.identity.profile.alias.clone(),
                local_services: services.all().into_iter().cloned().collect(),
                networks: networks.clone(),
                known_peers: ns.len().max(qos_peer_count),
                version: env!("CARGO_PKG_VERSION").to_string(),
                reputation_tier: rep_tier,
                reputation_score: rep_score,
                pouch_stats,
                network_qos,
            };
            ControlResponse::ok(data)
        }

        ControlRequest::AnnounceNow => {
            announce_all(state).await;
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            ControlResponse::ok("announced")
        }

        ControlRequest::Hatch {
            service_type,
            network_id,
            metadata,
        } => {
            if let Err(e) = subscribe(state, &network_id).await {
                return ControlResponse::err(e.to_string());
            }

            if service_type == ServiceType::Pouch {
                let already_exists =
                    state.services.read().unwrap().all().iter().any(|s| {
                        s.service_type == ServiceType::Pouch && s.network_id == network_id
                    });
                if already_exists {
                    return ControlResponse::err(format!(
                        "a Pouch for network '{}' already exists on this node; \
                         two correlated Pouches defeat distributed redundancy",
                        network_id
                    ));
                }
            }

            let info = ServiceInfo::new(service_type, network_id.clone(), metadata.clone());
            let service_id = info.id.clone();

            state.services.write().unwrap().register(info);

            if service_type == ServiceType::Pouch {
                let quota = metadata
                    .get("storage_bytes")
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

            announce_service_id(state, &service_id).await;

            ControlResponse::ok(HatchData {
                service_id: service_id.clone(),
                service_type,
                network_id,
                message: format!("🦤 {} service hatched — id: {}", service_type, service_id),
            })
        }

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
        ControlRequest::FarewellEvict { service_id } => {
            let Some(info) = state.services.read().unwrap().get(&service_id).cloned() else {
                return ControlResponse::err(format!("No service with id '{}'", service_id));
            };
            let (chunks, fragments, bytes) = evict_service(state, &info).await;
            ControlResponse::ok(serde_json::json!({
                "service_id": service_id,
                "network_id": info.network_id,
                "chunks_removed": chunks,
                "fragments_removed": fragments,
                "bytes_freed": bytes,
                "message": format!(
                    "Pouch {} evicted — {} chunk(s), {} fragment(s), {} bytes freed. \
                     Network peers will rebalance via Proof-of-Storage.",
                    service_id, chunks, fragments, bytes
                ),
            }))
        }
        ControlRequest::Pause {
            service_id,
            eta_minutes,
        } => {
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            {
                let mut reg = state.services.write().unwrap();
                match reg.get_mut(&service_id) {
                    Some(info) => {
                        if matches!(info.status, ServiceStatus::Paused { .. }) {
                            return ControlResponse::err(format!(
                                "Service '{}' is already paused",
                                service_id
                            ));
                        }
                        info.status = crate::service::ServiceStatus::Paused {
                            eta_minutes,
                            paused_at: now_secs,
                        };
                        info.metadata
                            .insert("maintenance".into(), serde_json::Value::Bool(true));
                        info.metadata
                            .insert("eta_minutes".into(), serde_json::Value::from(eta_minutes));
                    }
                    None => {
                        return ControlResponse::err(format!(
                            "No service with id '{}'",
                            service_id
                        ));
                    }
                }
            };

            announce_service_id(state, &service_id).await;

            ControlResponse::ok(serde_json::json!({
                "service_id": service_id,
                "status": "paused",
                "eta_minutes": eta_minutes,
                "message": format!(
                    "Service {} paused — announcing maintenance (ETA {} min)",
                    service_id, eta_minutes
                ),
            }))
        }

        ControlRequest::Resume { service_id } => {
            {
                let mut reg = state.services.write().unwrap();
                match reg.get_mut(&service_id) {
                    Some(info) => {
                        if !matches!(info.status, crate::service::ServiceStatus::Paused { .. }) {
                            return ControlResponse::err(format!(
                                "Service '{}' is not paused",
                                service_id
                            ));
                        }
                        info.status = crate::service::ServiceStatus::Running;
                        info.metadata.remove("maintenance");
                        info.metadata.remove("eta_minutes");
                    }
                    None => {
                        return ControlResponse::err(format!(
                            "No service with id '{}'",
                            service_id
                        ));
                    }
                }
            };

            announce_service_id(state, &service_id).await;

            ControlResponse::ok(serde_json::json!({
                "service_id": service_id,
                "status": "running",
                "message": format!("Service {} resumed", service_id),
            }))
        }

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

        ControlRequest::CreateNetwork { name } => {
            if state.memberships.read().unwrap().contains_key(&name) {
                return ControlResponse::err(format!("a network named '{name}' already exists"));
            }
            let result = Registry::create(&state.identity, &name).and_then(|(registry, _)| {
                registry.save()?;
                let id = registry.id()?;
                state
                    .memberships
                    .write()
                    .unwrap()
                    .insert(name.clone(), registry);
                Ok(id)
            });
            match result {
                Ok(id) => match subscribe(state, &name).await {
                    Ok(()) => ControlResponse::ok(serde_json::json!({
                        "network_id": name,
                        "genesis": id,
                        "message": format!("Created network '{name}'"),
                    })),
                    Err(e) => ControlResponse::err(e.to_string()),
                },
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }

        ControlRequest::Invite {
            network_id,
            member_key,
        } => {
            let result = {
                let mut memberships = state.memberships.write().unwrap();
                match memberships.get_mut(&network_id) {
                    None => Err(crate::error::BpError::InvalidInput(format!(
                        "you are not in a network named '{network_id}'"
                    ))),
                    Some(registry) => registry
                        .invite(&state.identity, &member_key)
                        .and_then(|()| registry.save())
                        .and_then(|()| {
                            Ok((
                                registry.to_token()?,
                                crate::identity::parse_user_key(&member_key)?.1,
                            ))
                        }),
                }
            };
            match result {
                Ok((token, member)) => {
                    publish_members(state, &network_id).await;
                    ControlResponse::ok(InviteData {
                        token,
                        network_id,
                        member,
                    })
                }
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }

        ControlRequest::JoinNetwork { token } => {
            let result = Registry::from_token(&token).and_then(|received| {
                if !received.is_member(&state.identity.fingerprint) {
                    return Err(crate::error::BpError::Identity(
                        "this invite is not for you".into(),
                    ));
                }
                let name = received.name()?.to_string();
                let mut memberships = state.memberships.write().unwrap();
                match memberships.get_mut(&name) {
                    Some(existing) => {
                        existing.merge(&received)?;
                        existing.save()?;
                    }
                    None => {
                        received.save()?;
                        memberships.insert(name.clone(), received);
                    }
                }
                Ok(name)
            });
            match result {
                Ok(name) => match subscribe(state, &name).await {
                    Ok(()) => {
                        publish_members(state, &name).await;
                        ControlResponse::ok(serde_json::json!({
                            "network_id": name,
                            "message": format!("Joined network '{name}'"),
                        }))
                    }
                    Err(e) => ControlResponse::err(e.to_string()),
                },
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }

        ControlRequest::Leave { network_id, force } => {
            let blocking: Vec<ServiceInfo> = {
                let reg = state.services.read().unwrap();
                reg.all()
                    .iter()
                    .filter(|s| s.network_id == network_id)
                    .map(|s| (*s).clone())
                    .collect()
            };

            if !blocking.is_empty() && !force {
                let blocking_json: Vec<serde_json::Value> = blocking
                    .iter()
                    .map(|s| {
                        let hint = if s.service_type == ServiceType::Pouch {
                            format!("bp farewell {} --evict", s.id)
                        } else {
                            format!("bp farewell {}", s.id)
                        };
                        serde_json::json!({
                            "id":   s.id,
                            "type": s.service_type.to_string(),
                            "hint": hint,
                        })
                    })
                    .collect();
                return ControlResponse::ok(serde_json::json!({
                    "network_id": network_id,
                    "blocked": true,
                    "blocking_services": blocking_json,
                    "message": format!(
                        "Cannot leave '{}': {} active service(s) must be stopped first \
                         (see 'blocking_services' for commands, or use --force to auto-evict)",
                        network_id,
                        blocking.len()
                    ),
                }));
            }

            let mut evicted: Vec<serde_json::Value> = Vec::new();
            for svc in &blocking {
                if svc.service_type == ServiceType::Pouch {
                    evict_service(state, svc).await;
                } else {
                    state.services.write().unwrap().remove(&svc.id);
                }
                tracing::info!(service_id = %svc.id, network_id = %network_id, "stopped by leave --force");
                evicted.push(serde_json::json!({
                    "service_id":   svc.id,
                    "service_type": svc.service_type.to_string(),
                    "evicted":      svc.service_type == ServiceType::Pouch,
                }));
            }

            let _ = state
                .net_tx
                .send(NetworkCommand::LeaveNetwork {
                    network_id: network_id.clone(),
                })
                .await;
            state.networks.write().unwrap().retain(|n| n != &network_id);

            let msg = if evicted.is_empty() {
                format!("Left network '{}'", network_id)
            } else {
                format!(
                    "Left network '{}' — {} service(s) auto-evicted",
                    network_id,
                    evicted.len()
                )
            };
            ControlResponse::ok(serde_json::json!({
                "network_id": network_id,
                "services_auto_evicted": evicted,
                "message": msg,
            }))
        }

        ControlRequest::ConnectRelay { relay_addr } => {
            let addr: Multiaddr = match relay_addr.parse() {
                Ok(a) => a,
                Err(e) => {
                    return ControlResponse::err(format!("Invalid relay multiaddr: {}", e));
                }
            };
            let _ = state
                .net_tx
                .send(NetworkCommand::DialRelay { relay_addr: addr })
                .await;
            ControlResponse::ok(serde_json::json!({
                "relay_addr": relay_addr,
                "message": format!("Dialing relay '{}'", relay_addr),
            }))
        }
        ControlRequest::PutFile {
            chunk_data,
            ph,
            q_target,
            network_id,
            file_name,
        } => {
            let ph = ph.unwrap_or(0.999);
            let q_target = q_target.unwrap_or(1.0);

            let managers_snap: Vec<(String, Arc<RwLock<StorageManager>>)> = {
                let map = state.storage_managers.read().unwrap();
                map.iter()
                    .map(|(id, sm)| (id.clone(), Arc::clone(sm)))
                    .collect()
            };

            let candidates: Vec<_> = managers_snap
                .iter()
                .filter(|(_, sm)| {
                    let meta = &sm.read().unwrap().meta;
                    network_id.is_empty() || meta.network_id == network_id
                })
                .collect();

            let has_local_pouch = !candidates.is_empty();

            let pouch_peer_ids: Vec<String> = {
                let ns = state.network_state.read().unwrap();
                ns.in_network(&network_id)
                    .into_iter()
                    .filter(|n| n.service_type == ServiceType::Pouch)
                    .map(|n| n.peer_id.clone())
                    .collect()
            };

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

            let plaintext_hash: [u8; 32] = *blake3::hash(&chunk_data).as_bytes();
            let secret_mat = state.identity.secret_material();
            let cipher = ChunkCipher::for_user(&secret_mat, &plaintext_hash);
            let encrypted_chunk = match cipher.encrypt(&chunk_data) {
                Ok(b) => b,
                Err(e) => return ControlResponse::err(format!("Chunk encryption error: {e}")),
            };

            let fragments = match rlnc::encode(&encrypted_chunk, k, n) {
                Ok(f) => f,
                Err(e) => return ControlResponse::err(format!("Encode error: {e}")),
            };

            let chunk_id = match fragments.first() {
                Some(f) => f.chunk_id.clone(),
                None => return ControlResponse::err("Encoding produced no fragments"),
            };

            state
                .chunk_cek_hints
                .write()
                .unwrap()
                .insert(chunk_id.clone(), plaintext_hash);
            {
                let hints = state.chunk_cek_hints.read().unwrap();
                persist_cek_hints(&hints);
            }

            let mut stored = 0usize;
            if has_local_pouch {
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
            }

            let remote_pouches: Vec<PeerId> = {
                let ns = state.network_state.read().unwrap();
                let local_peer = state.identity.peer_id.to_string();
                ns.in_network(&network_id)
                    .into_iter()
                    .filter(|n| n.service_type == ServiceType::Pouch && n.peer_id != local_peer)
                    .filter_map(|n| n.peer_id.parse::<PeerId>().ok())
                    .collect()
            };

            if stored == 0 && remote_pouches.is_empty() {
                return ControlResponse::err(format!(
                    "No Pouches available on network '{}' — hatch a Pouch or wait for Pouch peers to join",
                    network_id
                ));
            }

            let mut distributed = 0usize;
            let mut index_pointers: Vec<FragmentPointer> = Vec::new();
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
                        index_pointers.push(FragmentPointer {
                            peer_id: peer.to_string(),
                            fragment_id: fragment.id.clone(),
                        });
                    }
                }
            }

            if !index_pointers.is_empty() {
                let ann = FragmentIndexAnnouncement {
                    network_id: network_id.clone(),
                    chunk_id: chunk_id.clone(),
                    announced_at: chrono::Utc::now().timestamp() as u64,
                    pointers: index_pointers,
                };
                if let Ok(mut idx) = state.remote_fragment_index.write() {
                    idx.upsert(ann.clone());
                }
                let _ = state
                    .net_tx
                    .send(NetworkCommand::Publish {
                        network_id: network_id.clone(),
                        message: Gossip::Index(ann),
                    })
                    .await;
            }

            tracing::info!(
                chunk_id=%chunk_id, stored=%stored, distributed=%distributed,
                total=%fragments.len(), "PutFile stored + distributed"
            );

            if let Some(name) = file_name {
                let entry = crate::storage::StoredFileEntry {
                    file_name: name,
                    size_bytes: chunk_data.len() as u64,
                    chunk_id: chunk_id.clone(),
                    network_id: network_id.clone(),
                    uploaded_at: chrono::Utc::now().timestamp() as u64,
                };
                if let Ok(path) = crate::config::file_registry_path() {
                    state
                        .file_registry
                        .write()
                        .unwrap()
                        .insert_and_save(entry, &path);
                }
            }

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

            let remote_only = managers_snap.is_empty();

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

            let k_needed = all_fragments.first().map(|f| f.k).unwrap_or(0);

            let mut fragments_remote = 0usize;
            if remote_only || (k_needed > 0 && all_fragments.len() < k_needed) {
                let local_peer = state.identity.peer_id.to_string();

                let target_peers: Vec<PeerId> = {
                    let idx = state.remote_fragment_index.read().unwrap();
                    let ptrs = idx.pointers_for(&chunk_id);
                    if !ptrs.is_empty() {
                        let mut seen = std::collections::HashSet::new();
                        ptrs.iter()
                            .filter(|p| p.peer_id != local_peer)
                            .filter_map(|p| {
                                if seen.insert(p.peer_id.clone()) {
                                    p.peer_id.parse::<PeerId>().ok()
                                } else {
                                    None
                                }
                            })
                            .collect()
                    } else {
                        let ns = state.network_state.read().unwrap();
                        ns.in_network(&network_id)
                            .into_iter()
                            .filter(|n| {
                                n.service_type == ServiceType::Pouch && n.peer_id != local_peer
                            })
                            .filter_map(|n| n.peer_id.parse::<PeerId>().ok())
                            .collect()
                    }
                };

                let mut receivers = Vec::new();
                for peer in &target_peers {
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
                        Ok(Ok(_)) => {}
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
                Ok(encrypted_data) => {
                    let plaintext_hash = {
                        state
                            .chunk_cek_hints
                            .read()
                            .unwrap()
                            .get(&chunk_id)
                            .copied()
                    };
                    let data = match plaintext_hash {
                        Some(ph) => {
                            let secret_mat = state.identity.secret_material();
                            let cipher = ChunkCipher::for_user(&secret_mat, &ph);
                            match cipher.decrypt(&encrypted_data) {
                                Ok(d) => d,
                                Err(e) => {
                                    return ControlResponse::err(format!(
                                        "Chunk decryption failed: {e}"
                                    ))
                                }
                            }
                        }
                        None => {
                            return ControlResponse::err(format!(
                                "CEK hint not found for chunk '{}' — daemon may have restarted",
                                chunk_id
                            ))
                        }
                    };
                    tracing::info!(
                        chunk_id=%chunk_id, local=%local_count,
                        remote=%fragments_remote, total=%fragments_used,
                        "GetFile decoded + decrypted"
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

        ControlRequest::StorageInfo { network_id } => {
            let services = state.services.read().unwrap();
            let registry = state.file_registry.read().unwrap();

            let pouches = pouch_stats(state, &services, &network_id);

            let total_bid_bytes: u64 = pouches.iter().map(|p| p.storage_bid_bytes).sum();
            let total_used_bytes: u64 = pouches.iter().map(|p| p.storage_used_bytes).sum();
            let total_available_bytes: u64 = pouches.iter().map(|p| p.available_bytes).sum();

            let net_filter = if network_id.is_empty() {
                ""
            } else {
                &network_id
            };
            let file_entries = registry.list(net_filter);
            let total_files_uploaded = file_entries.len();
            let total_uploaded_bytes: u64 = file_entries.iter().map(|e| e.size_bytes).sum();

            ControlResponse::ok(StorageInfoData {
                pouches,
                total_bid_bytes,
                total_used_bytes,
                total_available_bytes,
                total_files_uploaded,
                total_uploaded_bytes,
            })
        }

        ControlRequest::ListFiles { network_id } => {
            let registry = state.file_registry.read().unwrap();
            let net_filter = if network_id.is_empty() {
                ""
            } else {
                &network_id
            };
            let entries: Vec<FileEntry> = registry
                .list(net_filter)
                .into_iter()
                .map(|e| FileEntry {
                    file_name: e.file_name.clone(),
                    size_bytes: e.size_bytes,
                    chunk_id: e.chunk_id.clone(),
                    network_id: e.network_id.clone(),
                    uploaded_at: e.uploaded_at,
                })
                .collect();
            let total_bytes = entries.iter().map(|e| e.size_bytes).sum();
            let total_files = entries.len();
            ControlResponse::ok(ListFilesData {
                files: entries,
                network_id: network_id.clone(),
                total_files,
                total_bytes,
            })
        }
    }
}

fn availability_factor(state: &DaemonState) -> f64 {
    let own_peer = state.identity.peer_id.to_string();
    let tier = state.reputation.read().unwrap().tier(&own_peer);
    if tier == crate::network::ReputationTier::R0 {
        return 0.0;
    }
    let qos = state.qos.read().unwrap();
    let own_score = qos.get(&own_peer).map_or(0.4, |q| q.stability_score());
    let mut stabilities = qos.all_stability_scores();
    stabilities.push(own_score);
    coding_params::compute_network_storage_factor(&stabilities, tier.qos_target_ph())
}

fn pouch_stats(
    state: &DaemonState,
    services: &ServiceRegistry,
    network_id: &str,
) -> Vec<PouchStat> {
    let factor = availability_factor(state);
    let managers = state.storage_managers.read().unwrap();
    services
        .all()
        .iter()
        .filter(|s| {
            s.service_type == ServiceType::Pouch
                && (network_id.is_empty() || s.network_id == network_id)
        })
        .map(|s| {
            let storage_tier = s
                .metadata
                .get("tier")
                .and_then(|v| v.as_str())
                .and_then(|t| StorageTier::parse(t).ok())
                .map(|t| format!("{t:?} — {}", t.name()));
            let (bid, used, avail) = managers.get(&s.id).map_or((0, 0, 0), |sm| {
                let sm = sm.read().unwrap();
                (
                    sm.meta.storage_bytes_bid,
                    sm.meta.storage_bytes_used,
                    (sm.meta.available_bytes() as f64 * factor).round() as u64,
                )
            });
            PouchStat {
                service_id: s.id.clone(),
                network_id: s.network_id.clone(),
                storage_tier,
                storage_bid_bytes: bid,
                storage_used_bytes: used,
                available_bytes: avail,
            }
        })
        .collect()
}

async fn announce_service(state: &DaemonState, svc: &ServiceInfo) {
    let info = NodeInfo {
        peer_id: state.identity.peer_id.to_string(),
        user_fingerprint: state.identity.fingerprint.clone(),
        user_alias: state.identity.profile.alias.clone(),
        service_type: svc.service_type,
        service_id: svc.id.clone(),
        network_id: svc.network_id.clone(),
        listen_addrs: vec![],
        announced_at: chrono::Utc::now().timestamp() as u64,
        metadata: svc.metadata.clone(),
        device_cert: state.identity.cert.clone(),
    };
    let _ = state
        .net_tx
        .send(NetworkCommand::Publish {
            network_id: svc.network_id.clone(),
            message: Gossip::Node(Box::new(info)),
        })
        .await;
}

async fn subscribe(state: &DaemonState, network_id: &str) -> BpResult<()> {
    let key = match state.memberships.read().unwrap().get(network_id) {
        Some(registry) => registry.network_key(&state.identity)?,
        None => {
            return Err(crate::error::BpError::InvalidInput(format!(
                "you are not in a network named '{network_id}' — create it or join with an invite"
            )))
        }
    };
    if !state
        .networks
        .read()
        .unwrap()
        .iter()
        .any(|n| n == network_id)
    {
        let _ = state
            .net_tx
            .send(NetworkCommand::JoinNetwork {
                network_id: network_id.to_string(),
                key,
            })
            .await;
        state.networks.write().unwrap().push(network_id.to_string());
    }
    Ok(())
}

async fn publish_members(state: &DaemonState, network_id: &str) {
    let entries = state
        .memberships
        .read()
        .unwrap()
        .get(network_id)
        .map(|r| r.entries().cloned().collect::<Vec<_>>());
    if let Some(entries) = entries {
        let _ = state
            .net_tx
            .send(NetworkCommand::Publish {
                network_id: network_id.to_string(),
                message: Gossip::Members { entries },
            })
            .await;
    }
}

pub async fn join_all(state: &DaemonState) {
    let names: Vec<String> = state.memberships.read().unwrap().keys().cloned().collect();
    for name in names {
        if let Err(e) = subscribe(state, &name).await {
            tracing::warn!(network = %name, "cannot join network: {e}");
        }
    }
}

async fn announce_service_id(state: &DaemonState, service_id: &str) {
    let svc = state.services.read().unwrap().get(service_id).cloned();
    if let Some(svc) = svc {
        announce_service(state, &svc).await;
    }
}

pub async fn announce_all(state: &DaemonState) {
    let services: Vec<ServiceInfo> = state
        .services
        .read()
        .unwrap()
        .all()
        .into_iter()
        .cloned()
        .collect();
    for svc in &services {
        announce_service(state, svc).await;
    }
    let networks: Vec<String> = state.networks.read().unwrap().clone();
    for network in networks {
        publish_members(state, &network).await;
    }
}

async fn evict_service(state: &DaemonState, svc: &ServiceInfo) -> (usize, usize, u64) {
    let summary = match state.storage_managers.write().unwrap().remove(&svc.id) {
        Some(sm) => {
            let sm = sm.read().unwrap();
            let summary = sm.storage_summary();
            if let Err(e) = sm.purge() {
                tracing::warn!("Failed to purge storage for {}: {}", svc.id, e);
            }
            summary
        }
        None => (0, 0, 0),
    };

    let mut notice = svc.clone();
    notice
        .metadata
        .insert("evicting".into(), serde_json::Value::Bool(true));
    announce_service(state, &notice).await;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    state
        .reputation
        .write()
        .unwrap()
        .get_or_create(&state.identity.peer_id.to_string(), now)
        .evict_without_notice(now);
    state.services.write().unwrap().remove(&svc.id);

    let (chunks, fragments, bytes) = summary;
    tracing::info!(service_id = %svc.id, chunks, fragments, bytes, "Service evicted");
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn load_cek_hints_missing_file_returns_empty() {
        let dir = tempdir();
        let path = dir.join("cek_hints.json");
        let hints = load_cek_hints_at(&path);
        assert!(hints.is_empty(), "expected empty map for missing file");
    }

    #[test]
    fn cek_hints_persist_load_roundtrip() {
        let dir = tempdir();
        let path = dir.join("cek_hints.json");

        let mut hints: HashMap<String, [u8; 32]> = HashMap::new();
        let key_a = *b"abcdefghijklmnopqrstuvwxyz012345";
        let key_b = *b"ABCDEFGHIJKLMNOPQRSTUVWXYZ678901";
        hints.insert("chunk-aaa".to_string(), key_a);
        hints.insert("chunk-bbb".to_string(), key_b);

        persist_cek_hints_at(&path, &hints);
        let loaded = load_cek_hints_at(&path);

        assert_eq!(loaded.len(), 2, "wrong number of hints loaded");
        assert_eq!(loaded["chunk-aaa"], key_a);
        assert_eq!(loaded["chunk-bbb"], key_b);
    }

    #[test]
    fn load_cek_hints_corrupt_file_returns_empty() {
        let dir = tempdir();
        let path = dir.join("cek_hints.json");
        std::fs::write(&path, b"NOT_VALID_JSON").unwrap();

        let hints = load_cek_hints_at(&path);
        assert!(hints.is_empty(), "corrupt file must yield empty map");
    }

    fn tempdir() -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("bp_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
