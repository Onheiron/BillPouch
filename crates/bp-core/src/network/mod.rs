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
pub use reputation::ReputationTier;
pub use state::{NetworkState, NodeInfo};

use crate::{
    chain::{now_ms, Chains, ObservationLog},
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

const CONSENSUS_TICK_MS: u64 = 200;
const SYNC_INTERVAL_SECS: u64 = 15;
const OLD_TOPIC_GRACE_MS: u64 = 600_000;

struct Subscription {
    network: String,
    key: [u8; 32],
    retire_at: Option<u64>,
}

type Topics = HashMap<gossipsub::TopicHash, Subscription>;

fn switch_key(
    swarm: &mut Swarm<BillPouchBehaviour>,
    networks: &mut HashMap<String, ([u8; 32], gossipsub::IdentTopic)>,
    topics: &mut Topics,
    network_id: &str,
    key: [u8; 32],
    now_ms: u64,
) {
    let topic = gossipsub::IdentTopic::new(crate::membership::gossip_topic(&key));
    if let Err(e) = swarm.behaviour_mut().gossipsub.subscribe(&topic) {
        tracing::warn!("Failed to subscribe to {}: {}", network_id, e);
    }
    for sub in topics.values_mut() {
        if sub.network == network_id && sub.retire_at.is_none() {
            sub.retire_at = Some(now_ms + OLD_TOPIC_GRACE_MS);
        }
    }
    topics.insert(
        topic.hash(),
        Subscription {
            network: network_id.to_string(),
            key,
            retire_at: None,
        },
    );
    networks.insert(network_id.to_string(), (key, topic));
}

pub type StorageManagerMap = Arc<RwLock<HashMap<String, Arc<RwLock<StorageManager>>>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingFragment {
    pub chunk_id: String,
    pub fragment_id: String,
    pub grant: crate::files::Grant,
    pub root: [u8; 32],
    pub blocks: usize,
    pub size: u64,
}

pub type OutgoingAssignments = Arc<RwLock<HashMap<String, Vec<OutgoingFragment>>>>;

pub fn load_outgoing() -> OutgoingAssignments {
    let placed = crate::config::outgoing_path()
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    Arc::new(RwLock::new(placed))
}

pub fn save_outgoing(outgoing: &OutgoingAssignments) {
    let result = crate::config::outgoing_path().and_then(|p| {
        let bytes = serde_json::to_vec(&*outgoing.read().unwrap())?;
        std::fs::write(p, bytes)?;
        Ok(())
    });
    if let Err(e) = result {
        tracing::warn!("cannot save fragment placements: {e}");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Gossip {
    Node(Box<NodeInfo>),
    Index(fragment_gossip::FragmentIndexAnnouncement),
    Consensus {
        msg: crate::consensus::ConsensusMsg,
    },
    Head {
        head: Box<crate::files::Head>,
    },
    HeadRequest {
        owner: String,
    },
    Share {
        to: String,
        manifest: Box<crate::files::Manifest>,
    },
    ShareAck {
        file_id: String,
        member: String,
    },
    FillerEvicted {
        blobs: Vec<String>,
    },
    Order {
        order: Box<crate::files::Order>,
    },
}

pub type Fillers = Arc<RwLock<crate::storage::filler::FillerBook>>;

pub type FileGossip = mpsc::Sender<(String, Gossip)>;

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
        grant: crate::files::Grant,
    },
    FetchChunkFragments {
        peer_id: PeerId,
        chunk_id: String,
        grant: crate::files::Grant,
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
        target: crate::storage::PosTarget,
        index: usize,
        resp_tx: tokio::sync::oneshot::Sender<Option<crate::storage::merkle::Proof>>,
    },
    Fill {
        peer_id: PeerId,
        network: String,
        blob_id: String,
        data: Vec<u8>,
        resp_tx: tokio::sync::oneshot::Sender<bool>,
    },
}

type PendingProofs = HashMap<
    request_response::OutboundRequestId,
    tokio::sync::oneshot::Sender<Option<crate::storage::merkle::Proof>>,
>;
type PendingFills =
    HashMap<request_response::OutboundRequestId, tokio::sync::oneshot::Sender<bool>>;

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
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
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
    chains: Chains,
    identity: crate::identity::Identity,
    observations: ObservationLog,
    file_gossip: FileGossip,
    fillers: Fillers,
) -> BpResult<()> {
    swarm
        .listen_on(listen_addr.clone())
        .map_err(|e| BpError::Network(e.to_string()))?;

    tracing::info!("Network listening on {}", listen_addr);

    let mut networks: HashMap<String, ([u8; 32], gossipsub::IdentTopic)> = HashMap::new();
    let mut topics: Topics = HashMap::new();

    let mut pending_fetches: HashMap<
        request_response::OutboundRequestId,
        tokio::sync::oneshot::Sender<FragmentResponse>,
    > = HashMap::new();

    let mut pending_pings: HashMap<
        request_response::OutboundRequestId,
        (u64, tokio::sync::oneshot::Sender<u64>),
    > = HashMap::new();

    let mut pending_pos: PendingProofs = HashMap::new();
    let mut pending_fills: PendingFills = HashMap::new();

    let mut pending_syncs: HashMap<request_response::OutboundRequestId, String> = HashMap::new();

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
    let mut consensus_interval = tokio::time::interval(Duration::from_millis(CONSENSUS_TICK_MS));
    let mut sync_interval = tokio::time::interval(Duration::from_secs(SYNC_INTERVAL_SECS));

    loop {
        tokio::select! {
            event = swarm.select_next_some() => {
                handle_swarm_event(event, &mut swarm, &state, &networks, &topics, &chains, &identity, &file_gossip, &storage_managers, &mut pending_fetches, &mut pending_pings, &mut pending_pos, &mut pending_fills, &fillers, &mut pending_syncs, &remote_fragment_index, &mut local_kad_peers).await;
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    None => {
                        tracing::info!("Network command channel closed — shutting down");
                        save_kad_peers(&local_kad_peers);
                        break;
                    }
                    Some(NetworkCommand::JoinNetwork { network_id, key }) => {
                        if !networks.contains_key(&network_id) {
                            switch_key(&mut swarm, &mut networks, &mut topics, &network_id, key, now_ms());
                            tracing::info!("Joined network gossip: {}", network_id);
                        }
                    }
                    Some(NetworkCommand::LeaveNetwork { network_id }) => {
                        networks.remove(&network_id);
                        topics.retain(|hash, sub| {
                            let keep = sub.network != network_id;
                            if !keep {
                                let _ = swarm.behaviour_mut().gossipsub.unsubscribe(&gossipsub::IdentTopic::new(hash.as_str()));
                            }
                            keep
                        });
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
                        grant,
                    }) => {
                        let root = crate::storage::merkle::root(&data);
                        let blocks = crate::storage::merkle::block_count(data.len());
                        let size = data.len() as u64;
                        let req = FragmentRequest::Store {
                            chunk_id: chunk_id.clone(),
                            fragment_id: fragment_id.clone(),
                            data,
                            grant: grant.clone(),
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
                                    grant,
                                    root,
                                    blocks,
                                    size,
                                });
                        }
                        tracing::debug!(peer=%peer_id, chunk=%chunk_id, frag=%fragment_id, "PushFragment sent");
                    }
                    Some(NetworkCommand::FetchChunkFragments {
                        peer_id,
                        chunk_id,
                        grant,
                        resp_tx,
                    }) => {
                        let req = FragmentRequest::FetchChunkFragments {
                            chunk_id: chunk_id.clone(),
                            grant,
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
                        target,
                        index,
                        resp_tx,
                    }) => {
                        let req_id = swarm
                            .behaviour_mut()
                            .fragment_exchange
                            .send_request(&peer_id, FragmentRequest::ProofOfStorage { target, index });
                        pending_pos.insert(req_id, resp_tx);
                    }
                    Some(NetworkCommand::Fill {
                        peer_id,
                        network,
                        blob_id,
                        data,
                        resp_tx,
                    }) => {
                        let req_id = swarm
                            .behaviour_mut()
                            .fragment_exchange
                            .send_request(&peer_id, FragmentRequest::Fill { network, blob_id, data });
                        pending_fills.insert(req_id, resp_tx);
                    }
                }
            }
            _ = kad_save_interval.tick() => {
                save_kad_peers(&local_kad_peers);
            }
            _ = consensus_interval.tick() => {
                let now = now_ms();
                let outgoing: Vec<(String, [u8; 32], Vec<crate::consensus::ConsensusMsg>)> = match chains.write() {
                    Ok(mut all) => networks
                        .keys()
                        .filter_map(|name| {
                            let chain = all.get_mut(name)?;
                            let held = holdings(&storage_managers, name);
                            chain.tick(&observations, &held, now);
                            Some((name.clone(), chain.key, chain.outbox()))
                        })
                        .collect(),
                    Err(_) => Vec::new(),
                };
                for (name, key, msgs) in outgoing {
                    if networks.get(&name).is_some_and(|(current, _)| *current != key) {
                        switch_key(&mut swarm, &mut networks, &mut topics, &name, key, now);
                        tracing::info!(network = %name, "Moved to the rotated gossip topic");
                    }
                    for msg in msgs {
                        publish(&mut swarm, &networks, &name, &Gossip::Consensus { msg });
                    }
                }
                topics.retain(|hash, sub| {
                    let keep = sub.retire_at.is_none_or(|at| now < at);
                    if !keep {
                        let _ = swarm.behaviour_mut().gossipsub.unsubscribe(&gossipsub::IdentTopic::new(hash.as_str()));
                    }
                    keep
                });
            }
            _ = sync_interval.tick() => {
                let peers: Vec<PeerId> = swarm.connected_peers().copied().collect();
                let wanted: Vec<(String, String, u64)> = chains
                    .read()
                    .map(|all| {
                        networks
                            .keys()
                            .filter_map(|name| all.get(name).map(|c| (name.clone(), c.id(), c.height())))
                            .collect()
                    })
                    .unwrap_or_default();
                for (name, id, height) in wanted {
                    for peer in &peers {
                        let req = FragmentRequest::Commits {
                            network: id.clone(),
                            from: height,
                            cert: identity.cert.clone(),
                        };
                        let req_id = swarm.behaviour_mut().fragment_exchange.send_request(peer, req);
                        pending_syncs.insert(req_id, name.clone());
                    }
                }
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
    topics: &Topics,
    chains: &Chains,
    identity: &crate::identity::Identity,
    file_gossip: &FileGossip,
    storage_managers: &StorageManagerMap,
    pending_fetches: &mut HashMap<
        request_response::OutboundRequestId,
        tokio::sync::oneshot::Sender<FragmentResponse>,
    >,
    pending_pings: &mut HashMap<
        request_response::OutboundRequestId,
        (u64, tokio::sync::oneshot::Sender<u64>),
    >,
    pending_pos: &mut PendingProofs,
    pending_fills: &mut PendingFills,
    fillers: &Fillers,
    pending_syncs: &mut HashMap<request_response::OutboundRequestId, String>,
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
            let Some(Subscription {
                network: network_id,
                key,
                ..
            }) = topics.get(&message.topic)
            else {
                return;
            };
            let Some(gossip) = crate::membership::open(key, &message.data)
                .ok()
                .and_then(|plain| serde_json::from_slice::<Gossip>(&plain).ok())
            else {
                tracing::warn!(peer=%propagation_source, "Dropped undecryptable gossip");
                return;
            };
            if matches!(
                gossip,
                Gossip::Head { .. }
                    | Gossip::HeadRequest { .. }
                    | Gossip::Share { .. }
                    | Gossip::ShareAck { .. }
                    | Gossip::Order { .. }
            ) {
                if file_gossip.try_send((network_id.clone(), gossip)).is_err() {
                    tracing::warn!("file gossip queue full, dropped a message");
                }
                return;
            }
            let behind = handle_gossip(
                gossip,
                network_id,
                message.source.as_ref(),
                swarm,
                networks,
                state,
                chains,
                fillers,
                remote_fragment_index,
            );
            if let Some(from) = behind {
                let req = FragmentRequest::Commits {
                    network: from.0,
                    from: from.1,
                    cert: identity.cert.clone(),
                };
                let req_id = swarm
                    .behaviour_mut()
                    .fragment_exchange
                    .send_request(&propagation_source, req);
                pending_syncs.insert(req_id, network_id.clone());
            }
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
            let requester = member_device(chains, &peer);
            let response = if let FragmentRequest::Commits {
                network,
                from,
                cert,
            } = &request
            {
                serve_commits(chains, &peer, network, *from, cert)
            } else if let Some(user) = requester {
                let (response, evicted) =
                    serve_fragment_request(&request, storage_managers, chains, &user);
                for (network, blobs) in evicted {
                    publish(swarm, networks, &network, &Gossip::FillerEvicted { blobs });
                }
                response
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
            if let Some(tx) = pending_pos.remove(&request_id) {
                let proof = match response {
                    FragmentResponse::ProofOfStorageOk { proof } => Some(proof),
                    _ => None,
                };
                let _ = tx.send(proof);
                return;
            }
            if let Some(tx) = pending_fills.remove(&request_id) {
                let _ = tx.send(matches!(response, FragmentResponse::Stored));
                return;
            }
            if let Some(network) = pending_syncs.remove(&request_id) {
                if let FragmentResponse::Commits { commits } = response {
                    let msgs = chains
                        .write()
                        .ok()
                        .and_then(|mut all| {
                            let chain = all.get_mut(&network)?;
                            chain.apply_synced(commits, now_ms());
                            Some(chain.outbox())
                        })
                        .unwrap_or_default();
                    for msg in msgs {
                        publish(swarm, networks, &network, &Gossip::Consensus { msg });
                    }
                }
                return;
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
            pending_syncs.remove(&request_id);
            if let Some(tx) = pending_pos.remove(&request_id) {
                let _ = tx.send(None);
            }
            if let Some(tx) = pending_fills.remove(&request_id) {
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
        Ok(payload) => match swarm
            .behaviour_mut()
            .gossipsub
            .publish(topic.clone(), payload)
        {
            Ok(_) | Err(gossipsub::PublishError::InsufficientPeers) => {}
            Err(e) => tracing::warn!("Gossip publish failed: {}", e),
        },
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
    chains: &Chains,
    fillers: &Fillers,
    remote_fragment_index: &Arc<RwLock<fragment_gossip::RemoteFragmentIndex>>,
) -> Option<(String, u64)> {
    match gossip {
        Gossip::Node(info) => {
            let member = chains
                .read()
                .map(|all| {
                    all.get(network_id)
                        .is_some_and(|c| c.is_member(&info.user_fingerprint))
                })
                .unwrap_or(false);
            if info.network_id == network_id && member && info.is_authentic(publisher) {
                if let Ok(mut st) = state.write() {
                    st.upsert(*info);
                }
            } else {
                tracing::debug!(claimed=%info.peer_id, "Dropped NodeInfo from a non-member or with an invalid certificate");
            }
            None
        }
        Gossip::Index(ann) => {
            let known = publisher.is_some_and(|p| member_device(chains, p).is_some());
            if known && ann.network_id == network_id {
                if let Ok(mut idx) = remote_fragment_index.write() {
                    idx.upsert(ann);
                }
            }
            None
        }
        Gossip::Consensus { msg } => {
            let (behind, msgs) = {
                let mut all = chains.write().ok()?;
                let chain = all.get_mut(network_id)?;
                let behind = chain
                    .handle(msg, now_ms())
                    .then(|| (chain.id(), chain.height()));
                (behind, chain.outbox())
            };
            for msg in msgs {
                publish(swarm, networks, network_id, &Gossip::Consensus { msg });
            }
            behind
        }
        Gossip::FillerEvicted { blobs } => {
            if let (Some(pouch), Ok(mut book)) = (publisher, fillers.write()) {
                book.forget(&pouch.to_string(), &blobs);
            }
            None
        }
        Gossip::Head { .. }
        | Gossip::HeadRequest { .. }
        | Gossip::Share { .. }
        | Gossip::ShareAck { .. }
        | Gossip::Order { .. } => None,
    }
}

fn holdings(
    storage_managers: &StorageManagerMap,
    network: &str,
) -> std::collections::BTreeMap<String, u64> {
    let mut held = std::collections::BTreeMap::new();
    if let Ok(managers) = storage_managers.read() {
        for sm in managers.values() {
            let sm = sm.read().unwrap();
            if sm.meta.network_id == network {
                for (owner_key, bytes) in sm.bytes_by_owner() {
                    if let Ok((_, owner)) = crate::identity::parse_user_key(&owner_key) {
                        *held.entry(owner).or_default() += bytes;
                    }
                }
            }
        }
    }
    held
}

fn member_device(chains: &Chains, peer: &PeerId) -> Option<String> {
    let peer = peer.to_string();
    let all = chains.read().ok()?;
    all.values().find_map(|c| {
        c.ledger()
            .device_owner(&peer)
            .filter(|user| c.is_member(user))
            .map(str::to_string)
    })
}

fn serve_commits(
    chains: &Chains,
    peer: &PeerId,
    network: &str,
    from: u64,
    cert: &crate::identity::DeviceCert,
) -> FragmentResponse {
    let Ok(device) = cert.verify() else {
        return FragmentResponse::NotFound;
    };
    let Ok(all) = chains.read() else {
        return FragmentResponse::NotFound;
    };
    match all.values().find(|c| c.id() == network) {
        Some(chain) if device.peer_id == *peer && chain.is_member(&device.fingerprint) => {
            FragmentResponse::Commits {
                commits: chain.commits_from(from),
            }
        }
        _ => {
            tracing::warn!(peer=%peer, "Refused ledger sync to a non-member");
            FragmentResponse::NotFound
        }
    }
}

type Evictions = Vec<(String, Vec<String>)>;

fn serve_fragment_request(
    req: &FragmentRequest,
    storage_managers: &StorageManagerMap,
    chains: &Chains,
    requester: &str,
) -> (FragmentResponse, Evictions) {
    let may_store = |network: &str| {
        chains.read().is_ok_and(|all| {
            all.get(network)
                .is_some_and(|c| c.ledger().snapshot.may_store(requester))
        })
    };
    let managers = storage_managers.read().unwrap();
    let mut evictions = Vec::new();
    let response = match req {
        FragmentRequest::FetchChunkFragments { chunk_id, grant } => {
            let mut fragments = Vec::new();
            for sm_arc in managers.values() {
                let sm = sm_arc.read().unwrap();
                let allowed = sm.owner_of(chunk_id).is_some_and(|o| {
                    grant.allows(&o.file_id, &o.owner_key, requester)
                        && !sm.is_revoked(&o.file_id, requester)
                });
                if !allowed {
                    continue;
                }
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
            grant,
        } => {
            use crate::coding::rlnc::EncodedFragment;
            match EncodedFragment::from_bytes(fragment_id.clone(), chunk_id.clone(), data) {
                _ if !grant.is_owner_claim() || grant.grantee != requester => {
                    FragmentResponse::StoreFailed {
                        reason: "a fragment can only be stored by a device of its owner".into(),
                    }
                }
                Ok(fragment) => {
                    let owner = crate::storage::ChunkOwner {
                        owner_key: grant.owner_key.clone(),
                        file_id: grant.file_id.clone(),
                    };
                    let stored = managers.values().find_map(|sm_arc| {
                        let mut sm = sm_arc.write().unwrap();
                        if !may_store(&sm.meta.network_id) {
                            tracing::info!(owner = %requester, "refused a fragment: storage above credit");
                            return None;
                        }
                        match sm.store_fragment(&fragment, &owner) {
                            Ok(evicted) => Some((sm.meta.network_id.clone(), evicted)),
                            Err(e) => {
                                tracing::debug!("Store attempt failed: {e}");
                                None
                            }
                        }
                    });
                    match stored {
                        Some((network, evicted)) => {
                            if !evicted.is_empty() {
                                evictions.push((network, evicted));
                            }
                            FragmentResponse::Stored
                        }
                        None => FragmentResponse::StoreFailed {
                            reason: "No storage manager with capacity".into(),
                        },
                    }
                }
                Err(e) => FragmentResponse::StoreFailed {
                    reason: format!("Invalid fragment data: {e}"),
                },
            }
        }
        FragmentRequest::Fill {
            network,
            blob_id,
            data,
        } => {
            let stored = managers.values().any(|sm_arc| {
                let mut sm = sm_arc.write().unwrap();
                sm.meta.network_id == *network && sm.store_filler(blob_id, data).is_ok()
            });
            if stored {
                FragmentResponse::Stored
            } else {
                FragmentResponse::StoreFailed {
                    reason: "no room for filler".into(),
                }
            }
        }
        FragmentRequest::Ping { nonce } => FragmentResponse::Pong { nonce: *nonce },
        FragmentRequest::Commits { .. } => FragmentResponse::NotFound,
        FragmentRequest::ProofOfStorage { target, index } => managers
            .values()
            .find_map(|sm| sm.read().unwrap().prove(target, *index))
            .map_or(FragmentResponse::NotFound, |proof| {
                FragmentResponse::ProofOfStorageOk { proof }
            }),
    };
    (response, evictions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        identity::Identity,
        ledger::{Op, SignedOp},
        membership::genesis,
    };

    #[test]
    fn every_gossip_variant_survives_sealing() {
        let id = Identity::in_memory(None).unwrap();
        let (_, key) = genesis(&id, "amici").unwrap();
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
            Gossip::Consensus {
                msg: crate::consensus::ConsensusMsg::Op {
                    op: SignedOp::sign(
                        &id,
                        Op::RegisterDevice {
                            cert: id.cert.clone(),
                        },
                    )
                    .unwrap(),
                },
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
