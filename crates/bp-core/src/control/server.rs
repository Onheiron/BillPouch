use super::files;
use crate::{
    chain::{now_ms, Chain, Chains},
    coding::params as coding_params,
    control::protocol::{
        ControlRequest, ControlResponse, ExpelData, FileEntry, FlockData, HatchData, InviteData,
        MemberInfo, MembersData, NetworkQosSummary, PouchStat, StatusData, StorageInfoData,
    },
    error::BpResult,
    identity::Identity,
    membership::Invite,
    network::{
        fragment_gossip::RemoteFragmentIndex, state::NodeInfo, Gossip, NetworkCommand,
        OutgoingAssignments, QosRegistry, ReputationTier, StorageManagerMap,
    },
    service::{ServiceInfo, ServiceRegistry, ServiceStatus, ServiceType},
    storage::{StorageManager, StorageTier},
};
use libp2p::Multiaddr;
use std::{
    path::Path,
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::mpsc,
};

pub struct DaemonState {
    pub identity: Identity,
    pub services: RwLock<ServiceRegistry>,
    pub network_state: Arc<RwLock<crate::network::state::NetworkState>>,
    pub networks: RwLock<Vec<String>>,
    pub chains: Chains,
    pub net_tx: mpsc::Sender<NetworkCommand>,
    pub storage_managers: StorageManagerMap,
    pub qos: Arc<RwLock<QosRegistry>>,
    pub outgoing_assignments: OutgoingAssignments,
    pub remote_fragment_index: Arc<RwLock<RemoteFragmentIndex>>,
    pub files: RwLock<crate::files::FileStore>,
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

            let rep_tier = own_tier(state).to_string();
            let pouch_stats = pouch_stats(state, &services, "");

            let qos_peer_count = state.qos.read().unwrap().peer_count();
            let network_qos = {
                let qos = state.qos.read().unwrap();
                let chains = state.chains.read().unwrap();
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
                        let tier = chains
                            .values()
                            .filter_map(|c| c.device_tier(peer_id))
                            .max()
                            .map_or_else(|| "unknown".to_string(), |t| t.to_string());
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
            if !is_confirmed_member(state, &network_id) {
                return ControlResponse::err(format!(
                    "your membership of '{network_id}' is not in the ledger yet — wait for it to sync"
                ));
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
            if state.chains.read().unwrap().contains_key(&name) {
                return ControlResponse::err(format!("a network named '{name}' already exists"));
            }
            let result = Chain::create(&state.identity, &name).and_then(|mut chain| {
                chain.save()?;
                let id = chain.id();
                state.chains.write().unwrap().insert(name.clone(), chain);
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
            let member = match crate::identity::parse_user_key(&member_key) {
                Ok((_, fingerprint)) => fingerprint,
                Err(e) => return ControlResponse::err(e.to_string()),
            };
            if member == state.identity.fingerprint {
                let token = state
                    .chains
                    .read()
                    .unwrap()
                    .get(&network_id)
                    .ok_or_else(|| {
                        crate::error::BpError::InvalidInput(format!(
                            "you are not in a network named '{network_id}'"
                        ))
                    })
                    .and_then(|chain| chain.own_invite().to_token());
                return match token {
                    Ok(token) => ControlResponse::ok(InviteData {
                        token,
                        network_id,
                        member,
                        for_device: true,
                    }),
                    Err(e) => ControlResponse::err(e.to_string()),
                };
            }
            let result = match state.chains.write().unwrap().get_mut(&network_id) {
                None => Err(crate::error::BpError::InvalidInput(format!(
                    "you are not in a network named '{network_id}'"
                ))),
                Some(chain) if chain.is_member(&member) => Err(
                    crate::error::BpError::InvalidInput(format!("{member} is already a member")),
                ),
                Some(chain) => chain.invite(&member_key, now_ms()),
            };
            let token = match result.and_then(|invite| invite.to_token()) {
                Ok(token) => token,
                Err(e) => return ControlResponse::err(e.to_string()),
            };
            if wait_until(state, &network_id, |c| c.is_member(&member)).await {
                ControlResponse::ok(InviteData {
                    token,
                    network_id,
                    member,
                    for_device: false,
                })
            } else {
                ControlResponse::err(
                    "the invitation was not committed by the network in time — are enough members online?",
                )
            }
        }

        ControlRequest::JoinNetwork { token } => {
            let result = Invite::from_token(&token).and_then(|invite| {
                let name = invite.genesis.body.network.clone();
                if state.chains.read().unwrap().contains_key(&name) {
                    return Ok(name);
                }
                let mut chain = Chain::from_invite(&state.identity, invite)?;
                chain.save()?;
                state.chains.write().unwrap().insert(name.clone(), chain);
                Ok(name)
            });
            let name = match result {
                Ok(name) => name,
                Err(e) => return ControlResponse::err(e.to_string()),
            };
            if let Err(e) = subscribe(state, &name).await {
                return ControlResponse::err(e.to_string());
            }
            let me = state.identity.fingerprint.clone();
            if wait_until(state, &name, |c| c.is_member(&me) && c.key_is_current()).await {
                ControlResponse::ok(serde_json::json!({
                    "network_id": name,
                    "message": format!("Joined network '{name}'"),
                }))
            } else {
                ControlResponse::err(format!(
                    "joined '{name}' but could not sync its ledger from any member yet"
                ))
            }
        }

        ControlRequest::Members { network_id } => {
            let chains = state.chains.read().unwrap();
            let Some(chain) = chains.get(&network_id) else {
                return ControlResponse::err(format!(
                    "you are not in a network named '{network_id}'"
                ));
            };
            let ledger = chain.ledger();
            let members = ledger
                .active_members()
                .map(|(fingerprint, m)| MemberInfo {
                    fingerprint: fingerprint.clone(),
                    tier: ledger
                        .snapshot
                        .tier(fingerprint)
                        .map_or_else(|| "new".to_string(), |t| t.to_string()),
                    weight: ledger.snapshot.weight(fingerprint),
                    credit_bytes: ledger
                        .snapshot
                        .users
                        .get(fingerprint)
                        .map_or(0, |s| s.credit_bytes),
                    used_bytes: ledger
                        .snapshot
                        .users
                        .get(fingerprint)
                        .map_or(0, |s| s.used_bytes),
                    devices: ledger
                        .devices
                        .values()
                        .filter(|d| &d.user == fingerprint)
                        .count(),
                    joined_at: m.joined_at,
                    expel_approvals: ledger
                        .expel_approvals
                        .get(fingerprint)
                        .map_or(0, |a| a.len()),
                })
                .collect();
            ControlResponse::ok(MembersData {
                network_id: network_id.clone(),
                height: ledger.height,
                epoch: ledger.snapshot.epoch,
                total_weight: ledger.snapshot.total_weight(),
                members,
                expelled: ledger.expelled.iter().cloned().collect(),
            })
        }

        ControlRequest::Expel { network_id, member } => {
            let result = match state.chains.write().unwrap().get_mut(&network_id) {
                None => Err(crate::error::BpError::InvalidInput(format!(
                    "you are not in a network named '{network_id}'"
                ))),
                Some(chain) => chain.expel(&member, now_ms()),
            };
            if let Err(e) = result {
                return ControlResponse::err(e.to_string());
            }
            if !wait_until(state, &network_id, |c| c.expel_outcome(&member).is_some()).await {
                return ControlResponse::err(
                    "the expulsion vote was not committed by the network in time",
                );
            }
            let outcome = state.chains.read().unwrap()[&network_id]
                .expel_outcome(&member)
                .expect("checked above");
            ControlResponse::ok(ExpelData {
                network_id,
                member,
                outcome,
            })
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
            path,
            ph,
            q_target,
            network_id,
            file_name,
        } => files::put_file(state, path, ph, q_target, network_id, file_name).await,

        ControlRequest::GetFile {
            file_id,
            network_id,
            output,
        } => files::get_file(state, file_id, network_id, output).await,

        ControlRequest::DeleteFile {
            file_id,
            network_id,
        } => files::delete_file(state, network_id, file_id).await,

        ControlRequest::Share {
            network_id,
            file_id,
            member,
        } => files::share_file(state, network_id, file_id, member).await,

        ControlRequest::Unshare {
            network_id,
            file_id,
            member,
        } => files::unshare_file(state, network_id, file_id, member).await,

        ControlRequest::StorageInfo { network_id } => {
            let pouches = {
                let services = state.services.read().unwrap();
                pouch_stats(state, &services, &network_id)
            };
            let own: Vec<FileEntry> = files::list_files(state, &network_id)
                .files
                .into_iter()
                .filter(|f| f.mine)
                .collect();
            ControlResponse::ok(StorageInfoData {
                total_bid_bytes: pouches.iter().map(|p| p.storage_bid_bytes).sum(),
                total_used_bytes: pouches.iter().map(|p| p.storage_used_bytes).sum(),
                total_available_bytes: pouches.iter().map(|p| p.available_bytes).sum(),
                total_files_uploaded: own.len(),
                total_uploaded_bytes: own.iter().map(|f| f.size_bytes).sum(),
                pouches,
            })
        }

        ControlRequest::ListFiles { network_id } => {
            ControlResponse::ok(files::list_files(state, &network_id))
        }
    }
}

const MEMBERSHIP_WAIT_SECS: u64 = 90;

async fn wait_until(state: &DaemonState, network_id: &str, done: impl Fn(&Chain) -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(MEMBERSHIP_WAIT_SECS);
    loop {
        if state
            .chains
            .read()
            .unwrap()
            .get(network_id)
            .is_some_and(&done)
        {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

fn is_confirmed_member(state: &DaemonState, network_id: &str) -> bool {
    state
        .chains
        .read()
        .unwrap()
        .get(network_id)
        .is_some_and(|c| c.is_member(&state.identity.fingerprint))
}

fn own_tier(state: &DaemonState) -> ReputationTier {
    state
        .chains
        .read()
        .unwrap()
        .values()
        .filter_map(|c| c.tier(&state.identity.fingerprint))
        .max()
        .unwrap_or(ReputationTier::R1)
}

fn availability_factor(state: &DaemonState) -> f64 {
    let own_peer = state.identity.peer_id.to_string();
    let tier = own_tier(state);
    let qos = state.qos.read().unwrap();
    let own_score = qos.get(&own_peer).map_or(0.4, |q| q.stability_score());
    let mut stabilities = qos.all_stability_scores();
    stabilities.push(own_score);
    coding_params::storage_factor(&stabilities, tier.qos_target_ph())
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
                    ((sm.meta.available_bytes() + sm.filler_bytes()) as f64 * factor).round()
                        as u64,
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

fn pouch_metadata(
    state: &DaemonState,
    svc: &ServiceInfo,
) -> std::collections::HashMap<String, serde_json::Value> {
    let mut metadata = svc.metadata.clone();
    if let Some(sm) = state.storage_managers.read().unwrap().get(&svc.id) {
        let free = sm.read().unwrap().meta.available_bytes();
        metadata.insert("free_bytes".into(), serde_json::Value::from(free));
    }
    metadata
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
        metadata: pouch_metadata(state, svc),
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
    let key = match state.chains.read().unwrap().get(network_id) {
        Some(chain) => chain.key,
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

pub async fn join_all(state: &DaemonState) {
    let names: Vec<String> = state.chains.read().unwrap().keys().cloned().collect();
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

    state.services.write().unwrap().remove(&svc.id);

    let (chunks, fragments, bytes) = summary;
    tracing::info!(service_id = %svc.id, chunks, fragments, bytes, "Service evicted");
    summary
}
