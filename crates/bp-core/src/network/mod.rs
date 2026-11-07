pub mod behaviour;
pub mod bootstrap;
pub mod fragment_gossip;
pub mod kad_store;
pub mod qos;
pub mod quality_monitor;
pub mod reputation;
pub mod state;

pub use behaviour::{BillPouchBehaviour, FragmentRequest, FragmentResponse};
pub use bootstrap::BootstrapList;
pub use fragment_gossip::{FragmentIndexAnnouncement, FragmentPointer, RemoteFragmentIndex};
pub use kad_store::KadPeers;
pub use qos::{PeerQos, QosRegistry, FAULT_BLACKLISTED, FAULT_DEGRADED, FAULT_SUSPECTED};
pub use quality_monitor::run_quality_monitor;
pub use reputation::{ReputationRecord, ReputationStore, ReputationTier};
pub use state::{NetworkState, NodeInfo};

use crate::{
    error::{BpError, BpResult},
    storage::StorageManager,
};
use futures::StreamExt;
use libp2p::{gossipsub, request_response, swarm::SwarmEvent, Multiaddr, PeerId, Swarm};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;

pub type StorageManagerMap = Arc<RwLock<HashMap<String, Arc<RwLock<StorageManager>>>>>;

#[derive(Debug, Clone)]
pub struct OutgoingFragment {
    pub chunk_id: String,
    pub fragment_id: String,
}

pub type OutgoingAssignments = Arc<RwLock<HashMap<String, Vec<OutgoingFragment>>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Gossip {
    Node(Box<NodeInfo>),
    Index(fragment_gossip::FragmentIndexAnnouncement),
    Members {
        entries: Vec<crate::membership::Entry>,
    },
}

#[derive(Debug)]
pub enum NetworkCommand {
    JoinNetwork {
        network_id: String,
        key: [u8; 32],
    },
    LeaveNetwork {
        network_id: String,
    },
    Publish {
        network_id: String,
        message: Gossip,
    },
    Dial {
        addr: Multiaddr,
    },
    PushFragment {
        peer_id: PeerId,
        chunk_id: String,
        fragment_id: String,
        data: Vec<u8>,
    },
    FetchChunkFragments {
        peer_id: PeerId,
        chunk_id: String,
        resp_tx: tokio::sync::oneshot::Sender<FragmentResponse>,
    },
    DialRelay {
        relay_addr: Multiaddr,
    },
    Ping {
        peer_id: PeerId,
        sent_at_ms: u64,
        resp_tx: tokio::sync::oneshot::Sender<u64>,
    },
    ProofOfStorage {
        peer_id: PeerId,
        chunk_id: String,
        fragment_id: String,
        nonce: u64,
        resp_tx: tokio::sync::oneshot::Sender<bool>,
    },
}

pub fn build_swarm(
    keypair: libp2p::identity::Keypair,
) -> anyhow::Result<Swarm<BillPouchBehaviour>> {
    let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )?
        .with_relay_client(libp2p::noise::Config::new, libp2p::yamux::Config::default)?
        .with_behaviour(|key, relay_client| {
            BillPouchBehaviour::new(key, relay_client)
                .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e.to_string()))
        })
        .map_err(|e| anyhow::anyhow!("{}", e))?
        .build();

    Ok(swarm)
}

#[allow(clippy::too_many_arguments)]
pub async fn run_network_loop(
    mut swarm: Swarm<BillPouchBehaviour>,
    mut cmd_rx: mpsc::Receiver<NetworkCommand>,
    state: Arc<RwLock<NetworkState>>,
    listen_addr: Multiaddr,
    storage_managers: StorageManagerMap,
    outgoing_assignments: OutgoingAssignments,
    remote_fragment_index: Arc<RwLock<fragment_gossip::RemoteFragmentIndex>>,
    memberships: crate::membership::Memberships,
) -> BpResult<()> {
    swarm
        .listen_on(listen_addr.clone())
        .map_err(|e| BpError::Network(e.to_string()))?;

    tracing::info!("Network listening on {}", listen_addr);

    let mut networks: HashMap<String, ([u8; 32], gossipsub::IdentTopic)> = HashMap::new();
    let mut topics: HashMap<gossipsub::TopicHash, String> = HashMap::new();

    let mut pending_fetches: HashMap<
        request_response::OutboundRequestId,
        tokio::sync::oneshot::Sender<FragmentResponse>,
    > = HashMap::new();

    let mut pending_pings: HashMap<
        request_response::OutboundRequestId,
        (u64, tokio::sync::oneshot::Sender<u64>),
    > = HashMap::new();

    let mut pending_pos: HashMap<
        request_response::OutboundRequestId,
        tokio::sync::oneshot::Sender<bool>,
    > = HashMap::new();

    let mut local_kad_peers: kad_store::KadPeers = crate::config::kad_peers_path()
        .map(|p| kad_store::KadPeers::load(&p))
        .unwrap_or_default();

    if !local_kad_peers.0.is_empty() {
        tracing::info!(
            peers = local_kad_peers.0.len(),
            "kad_store: dialing saved peers"
        );
        for addrs in local_kad_peers.0.values() {
            for addr_str in addrs {
                if let Ok(addr) = addr_str.parse::<libp2p::Multiaddr>() {
                    let _ = swarm.dial(addr);
                }
            }
        }
    }

    if let Ok(bp_path) = crate::config::bootstrap_path() {
        bootstrap::BootstrapList::load(&bp_path).apply(&mut swarm);
    }

    const KAD_SAVE_INTERVAL_SECS: u64 = 600;
    let mut kad_save_interval = tokio::time::interval(Duration::from_secs(KAD_SAVE_INTERVAL_SECS));
    kad_save_interval.reset();

    loop {
        tokio::select! {
            event = swarm.select_next_some() => {
                handle_swarm_event(event, &mut swarm, &state, &networks, &topics, &memberships, &storage_managers, &mut pending_fetches, &mut pending_pings, &mut pending_pos, &remote_fragment_index, &mut local_kad_peers).await;
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    None => {
                        tracing::info!("Network command channel closed — shutting down");
                        save_kad_peers(&local_kad_peers);
                        break;
                    }
                    Some(NetworkCommand::JoinNetwork { network_id, key }) => {
                        let topic = gossipsub::IdentTopic::new(crate::membership::gossip_topic(&key));
                        if let Err(e) = swarm.behaviour_mut().gossipsub.subscribe(&topic) {
                            tracing::warn!("Failed to subscribe to {}: {}", network_id, e);
                        } else {
                            tracing::info!("Joined network gossip: {}", network_id);
                        }
                        topics.insert(topic.hash(), network_id.clone());
                        networks.insert(network_id, (key, topic));
                    }
                    Some(NetworkCommand::LeaveNetwork { network_id }) => {
                        if let Some((_, topic)) = networks.remove(&network_id) {
                            let _ = swarm.behaviour_mut().gossipsub.unsubscribe(&topic);
                            topics.remove(&topic.hash());
                        }
                    }
                    Some(NetworkCommand::Publish { network_id, message }) => {
                        publish(&mut swarm, &networks, &network_id, &message);
                    }
                    Some(NetworkCommand::Dial { addr }) => {
                        if let Err(e) = swarm.dial(addr.clone()) {
                            tracing::warn!("Dial {} failed: {}", addr, e);
                        }
                    }
                    Some(NetworkCommand::PushFragment {
                        peer_id,
                        chunk_id,
                        fragment_id,
                        data,
                    }) => {
                        let req = FragmentRequest::Store {
                            chunk_id: chunk_id.clone(),
                            fragment_id: fragment_id.clone(),
                            data,
                        };
                        let _ = swarm
                            .behaviour_mut()
                            .fragment_exchange
                            .send_request(&peer_id, req);
                        if let Ok(mut assignments) = outgoing_assignments.write() {
                            assignments
                                .entry(peer_id.to_string())
                                .or_default()
                                .push(OutgoingFragment {
                                    chunk_id: chunk_id.clone(),
                                    fragment_id: fragment_id.clone(),
                                });
                        }
                        tracing::debug!(peer=%peer_id, chunk=%chunk_id, frag=%fragment_id, "PushFragment sent");
                    }
                    Some(NetworkCommand::FetchChunkFragments {
                        peer_id,
                        chunk_id,
                        resp_tx,
                    }) => {
                        let req = FragmentRequest::FetchChunkFragments {
                            chunk_id: chunk_id.clone(),
                        };
                        let req_id = swarm
                            .behaviour_mut()
                            .fragment_exchange
                            .send_request(&peer_id, req);
                        pending_fetches.insert(req_id, resp_tx);
                        tracing::debug!(peer=%peer_id, chunk=%chunk_id, "FetchChunkFragments sent");
                    }
                    Some(NetworkCommand::DialRelay { relay_addr }) => {
                        if let Err(e) = swarm.dial(relay_addr.clone()) {
                            tracing::warn!(addr=%relay_addr, "Failed to dial relay: {}", e);
                        } else {
                            tracing::info!(addr=%relay_addr, "Dialing relay node for NAT traversal");
                        }
                    }
                    Some(NetworkCommand::Ping {
                        peer_id,
                        sent_at_ms,
                        resp_tx,
                    }) => {
                        let nonce = sent_at_ms;
                        let req = FragmentRequest::Ping { nonce };
                        let req_id = swarm
                            .behaviour_mut()
                            .fragment_exchange
                            .send_request(&peer_id, req);
                        pending_pings.insert(req_id, (sent_at_ms, resp_tx));
                        tracing::debug!(peer=%peer_id, nonce=%nonce, "Ping sent");
                    }
                    Some(NetworkCommand::ProofOfStorage {
                        peer_id,
                        chunk_id,
                        fragment_id,
                        nonce,
                        resp_tx,
                    }) => {
                        let req = FragmentRequest::ProofOfStorage {
                            chunk_id: chunk_id.clone(),
                            fragment_id: fragment_id.clone(),
                            nonce,
                        };
                        let req_id = swarm
                            .behaviour_mut()
                            .fragment_exchange
                            .send_request(&peer_id, req);
                        pending_pos.insert(req_id, resp_tx);
                        tracing::debug!(
                            peer = %peer_id,
                            chunk = %chunk_id,
                            frag = %fragment_id,
                            nonce = %nonce,
                            "ProofOfStorage challenge sent"
                        );
                    }
                }
            }
            _ = kad_save_interval.tick() => {
                save_kad_peers(&local_kad_peers);
            }
        }
    }

    Ok(())
}

fn save_kad_peers(peers: &kad_store::KadPeers) {
    if let Ok(path) = crate::config::kad_peers_path() {
        peers.save(&path);
        tracing::debug!(n = peers.0.len(), "kad_store: peers saved");
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_swarm_event(
    event: SwarmEvent<behaviour::BillPouchBehaviourEvent>,
    swarm: &mut Swarm<BillPouchBehaviour>,
    state: &Arc<RwLock<NetworkState>>,
    networks: &HashMap<String, ([u8; 32], gossipsub::IdentTopic)>,
    topics: &HashMap<gossipsub::TopicHash, String>,
    memberships: &crate::membership::Memberships,
    storage_managers: &StorageManagerMap,
    pending_fetches: &mut HashMap<
        request_response::OutboundRequestId,
        tokio::sync::oneshot::Sender<FragmentResponse>,
    >,
    pending_pings: &mut HashMap<
        request_response::OutboundRequestId,
        (u64, tokio::sync::oneshot::Sender<u64>),
    >,
    pending_pos: &mut HashMap<
        request_response::OutboundRequestId,
        tokio::sync::oneshot::Sender<bool>,
    >,
    remote_fragment_index: &Arc<RwLock<fragment_gossip::RemoteFragmentIndex>>,
    known_peers: &mut kad_store::KadPeers,
) {
    match event {
        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Gossipsub(
            gossipsub::Event::Message {
                propagation_source,
                message,
                ..
            },
        )) => {
            let Some(network_id) = topics.get(&message.topic) else {
                return;
            };
            let Some((key, _)) = networks.get(network_id) else {
                return;
            };
            let Some(gossip) = crate::membership::open(key, &message.data)
                .ok()
                .and_then(|plain| serde_json::from_slice::<Gossip>(&plain).ok())
            else {
                tracing::warn!(peer=%propagation_source, "Dropped undecryptable gossip");
                return;
            };
            handle_gossip(
                gossip,
                network_id,
                message.source.as_ref(),
                swarm,
                networks,
                state,
                memberships,
                remote_fragment_index,
            );
        }

        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Mdns(
            libp2p::mdns::Event::Discovered(list),
        )) => {
            for (peer_id, addr) in list {
                tracing::info!(peer=%peer_id, addr=%addr, "mDNS peer discovered");
                swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                known_peers.add(peer_id.to_string(), addr.to_string());
                swarm.behaviour_mut().kad.add_address(&peer_id, addr);
            }
        }

        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Mdns(
            libp2p::mdns::Event::Expired(list),
        )) => {
            for (peer_id, _addr) in list {
                tracing::debug!(peer=%peer_id, "mDNS peer expired");
                swarm
                    .behaviour_mut()
                    .gossipsub
                    .remove_explicit_peer(&peer_id);
            }
        }

        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Identify(
            libp2p::identify::Event::Received { peer_id, info, .. },
        )) => {
            tracing::debug!(peer=%peer_id, agent=%info.agent_version, "Identify received");
            for addr in info.listen_addrs {
                known_peers.add(peer_id.to_string(), addr.to_string());
                swarm.behaviour_mut().kad.add_address(&peer_id, addr);
            }
        }
        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Autonat(
            libp2p::autonat::Event::StatusChanged { old, new },
        )) => {
            tracing::info!(
                old = ?old,
                new = ?new,
                "AutoNAT: public reachability status changed"
            );
        }

        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Autonat(ev)) => {
            tracing::debug!(event = ?ev, "AutoNAT probe event");
        }

        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Relay(
            libp2p::relay::client::Event::ReservationReqAccepted { relay_peer_id, .. },
        )) => {
            tracing::info!(
                relay = %relay_peer_id,
                "Relay reservation accepted — node is reachable via relay"
            );
        }

        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Relay(
            libp2p::relay::client::Event::OutboundCircuitEstablished { relay_peer_id, .. },
        )) => {
            tracing::info!(
                relay = %relay_peer_id,
                "Relay outbound circuit established"
            );
        }

        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Relay(ev)) => {
            tracing::debug!(event = ?ev, "Relay client event");
        }
        SwarmEvent::NewListenAddr { address, .. } => {
            tracing::info!(addr=%address, "Now listening");
        }

        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            tracing::debug!(peer=%peer_id, "Connection established");
        }

        SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
            tracing::debug!(peer=%peer_id, cause=?cause, "Connection closed");
        }

        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::FragmentExchange(
            request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Request {
                        request, channel, ..
                    },
            },
        )) => {
            let known = state
                .read()
                .map(|s| s.knows(&peer.to_string()))
                .unwrap_or(false);
            let response = if known {
                serve_fragment_request(&request, storage_managers)
            } else {
                tracing::warn!(peer=%peer, "Refused fragment request from a non-member");
                FragmentResponse::NotFound
            };
            if let Err(e) = swarm
                .behaviour_mut()
                .fragment_exchange
                .send_response(channel, response)
            {
                tracing::warn!(peer=%peer, "send_response failed: {:?}", e);
            }
        }

        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::FragmentExchange(
            request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Response {
                        request_id,
                        response,
                    },
            },
        )) => {
            if let FragmentResponse::Pong { nonce } = &response {
                if let Some((sent_at_ms, tx)) = pending_pings.remove(&request_id) {
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let rtt_ms = now_ms.saturating_sub(sent_at_ms);
                    tracing::debug!(peer=%peer, nonce=%nonce, rtt_ms=%rtt_ms, "Pong received");
                    let _ = tx.send(rtt_ms);
                    return;
                }
            }
            if let FragmentResponse::ProofOfStorageOk { .. } = &response {
                if let Some(tx) = pending_pos.remove(&request_id) {
                    tracing::debug!(peer = %peer, "ProofOfStorage: proof received");
                    let _ = tx.send(true);
                    return;
                }
            }
            if let FragmentResponse::NotFound = &response {
                if let Some(tx) = pending_pos.remove(&request_id) {
                    tracing::debug!(peer = %peer, "ProofOfStorage: fragment not found");
                    let _ = tx.send(false);
                    return;
                }
            }
            if let Some(tx) = pending_fetches.remove(&request_id) {
                let _ = tx.send(response);
            } else {
                tracing::debug!(peer=%peer, "Untracked fragment response (fire-and-forget)");
            }
        }

        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::FragmentExchange(
            request_response::Event::OutboundFailure {
                request_id, error, ..
            },
        )) => {
            tracing::warn!("Fragment request failed: {:?}", error);
            pending_fetches.remove(&request_id);
            pending_pings.remove(&request_id);
            if let Some(tx) = pending_pos.remove(&request_id) {
                let _ = tx.send(false);
            }
        }

        _ => {}
    }
}

fn publish(
    swarm: &mut Swarm<BillPouchBehaviour>,
    networks: &HashMap<String, ([u8; 32], gossipsub::IdentTopic)>,
    network_id: &str,
    message: &Gossip,
) {
    let Some((key, topic)) = networks.get(network_id) else {
        tracing::warn!(network=%network_id, "Publish to a network we have not joined");
        return;
    };
    let sealed = serde_json::to_vec(message)
        .map_err(BpError::from)
        .and_then(|plain| crate::membership::seal(key, &plain));
    match sealed {
        Ok(payload) => {
            if let Err(e) = swarm
                .behaviour_mut()
                .gossipsub
                .publish(topic.clone(), payload)
            {
                tracing::warn!("Gossip publish failed: {}", e);
            }
        }
        Err(e) => tracing::warn!("Cannot seal gossip: {e}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_gossip(
    gossip: Gossip,
    network_id: &str,
    publisher: Option<&PeerId>,
    swarm: &mut Swarm<BillPouchBehaviour>,
    networks: &HashMap<String, ([u8; 32], gossipsub::IdentTopic)>,
    state: &Arc<RwLock<NetworkState>>,
    memberships: &crate::membership::Memberships,
    remote_fragment_index: &Arc<RwLock<fragment_gossip::RemoteFragmentIndex>>,
) {
    match gossip {
        Gossip::Node(info) => {
            let member = memberships
                .read()
                .map(|m| {
                    m.get(network_id)
                        .is_some_and(|r| r.is_member(&info.user_fingerprint))
                })
                .unwrap_or(false);
            if info.network_id == network_id && member && info.is_authentic(publisher) {
                if let Ok(mut st) = state.write() {
                    st.upsert(*info);
                }
            } else {
                tracing::warn!(claimed=%info.peer_id, "Dropped NodeInfo from a non-member or with an invalid certificate");
            }
        }
        Gossip::Index(ann) => {
            let known = publisher
                .zip(state.read().ok())
                .is_some_and(|(p, s)| s.knows(&p.to_string()));
            if known && ann.network_id == network_id {
                if let Ok(mut idx) = remote_fragment_index.write() {
                    idx.upsert(ann);
                }
            }
        }
        Gossip::Members { entries } => {
            let Ok(received) = crate::membership::Registry::from_entries(entries) else {
                return;
            };
            let reply = {
                let Ok(mut all) = memberships.write() else {
                    return;
                };
                let Some(local) = all.get_mut(network_id) else {
                    return;
                };
                match local.merge(&received) {
                    Ok(changed) => {
                        if changed {
                            if let Err(e) = local.save() {
                                tracing::warn!("Cannot save registry: {e}");
                            }
                        }
                        let theirs_is_missing =
                            local.entries().count() > received.entries().count();
                        theirs_is_missing.then(|| local.entries().cloned().collect())
                    }
                    Err(_) => None,
                }
            };
            if let Some(entries) = reply {
                publish(swarm, networks, network_id, &Gossip::Members { entries });
            }
        }
    }
}

fn serve_fragment_request(
    req: &FragmentRequest,
    storage_managers: &StorageManagerMap,
) -> FragmentResponse {
    let managers = storage_managers.read().unwrap();
    match req {
        FragmentRequest::FetchChunkFragments { chunk_id } => {
            let mut fragments = Vec::new();
            for sm_arc in managers.values() {
                let sm = sm_arc.read().unwrap();
                for meta in sm.index.fragments_for_chunk(chunk_id) {
                    match sm.load_fragment(chunk_id, &meta.fragment_id) {
                        Ok(f) => fragments.push((meta.fragment_id.clone(), f.to_bytes())),
                        Err(e) => tracing::warn!("serve_fragment load error: {e}"),
                    }
                }
            }
            if fragments.is_empty() {
                FragmentResponse::NotFound
            } else {
                FragmentResponse::FoundMany { fragments }
            }
        }
        FragmentRequest::Store {
            chunk_id,
            fragment_id,
            data,
        } => {
            use crate::coding::rlnc::EncodedFragment;
            match EncodedFragment::from_bytes(fragment_id.clone(), chunk_id.clone(), data) {
                Ok(fragment) => {
                    for sm_arc in managers.values() {
                        let mut sm = sm_arc.write().unwrap();
                        match sm.store_fragment(&fragment) {
                            Ok(()) => {
                                tracing::debug!(chunk=%chunk_id, frag=%fragment_id, "Remote fragment stored");
                                return FragmentResponse::Stored;
                            }
                            Err(e) => {
                                tracing::debug!("Store attempt failed: {e}");
                            }
                        }
                    }
                    FragmentResponse::StoreFailed {
                        reason: "No storage manager with capacity".into(),
                    }
                }
                Err(e) => FragmentResponse::StoreFailed {
                    reason: format!("Invalid fragment data: {e}"),
                },
            }
        }
        FragmentRequest::Ping { nonce } => FragmentResponse::Pong { nonce: *nonce },
        FragmentRequest::ProofOfStorage {
            chunk_id,
            fragment_id,
            nonce,
        } => {
            for sm_arc in managers.values() {
                let sm = sm_arc.read().unwrap();
                if sm
                    .index
                    .fragments_for_chunk(chunk_id)
                    .iter()
                    .any(|m| m.fragment_id == *fragment_id)
                {
                    match sm.load_fragment(chunk_id, fragment_id) {
                        Ok(fragment) => {
                            let data = fragment.to_bytes();
                            let mut hasher = blake3::Hasher::new();
                            hasher.update(&data);
                            hasher.update(&nonce.to_le_bytes());
                            let proof: [u8; 32] = hasher.finalize().into();
                            return FragmentResponse::ProofOfStorageOk { proof };
                        }
                        Err(e) => {
                            tracing::warn!(
                                chunk = %chunk_id,
                                frag = %fragment_id,
                                "serve_fragment PoS load error: {e}"
                            );
                        }
                    }
                }
            }
            FragmentResponse::NotFound
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{identity::Identity, membership::Registry};

    #[test]
    fn every_gossip_variant_survives_sealing() {
        let id = Identity::in_memory(None).unwrap();
        let (registry, key) = Registry::create(&id, "amici").unwrap();
        let messages = [
            Gossip::Node(Box::new(NodeInfo {
                peer_id: id.peer_id.to_string(),
                user_fingerprint: id.fingerprint.clone(),
                user_alias: None,
                service_type: crate::service::ServiceType::Bill,
                service_id: "svc".into(),
                network_id: "amici".into(),
                listen_addrs: vec![],
                announced_at: 0,
                metadata: HashMap::new(),
                device_cert: id.cert.clone(),
            })),
            Gossip::Index(fragment_gossip::FragmentIndexAnnouncement {
                network_id: "amici".into(),
                chunk_id: "c".into(),
                announced_at: 0,
                pointers: vec![],
            }),
            Gossip::Members {
                entries: registry.entries().cloned().collect(),
            },
        ];
        for message in messages {
            let sealed =
                crate::membership::seal(&key, &serde_json::to_vec(&message).unwrap()).unwrap();
            let opened: Gossip =
                serde_json::from_slice(&crate::membership::open(&key, &sealed).unwrap()).unwrap();
            assert_eq!(
                serde_json::to_value(&opened).unwrap(),
                serde_json::to_value(&message).unwrap()
            );
        }
    }
}
