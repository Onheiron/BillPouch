//! P2P networking layer built on libp2p.
//!
//! This module owns the libp2p [`Swarm`] and exposes two public entry points:
//!
//! - [`build_swarm`] — constructs the swarm with all four behaviours
//!   (gossipsub, Kademlia, Identify, mDNS) and the Noise+Yamux transport.
//! - [`run_network_loop`] — async task that drives the swarm event loop and
//!   processes [`NetworkCommand`]s sent by the daemon over a Tokio channel.
//!
//! ## Sub-modules
//!
//! - [`behaviour`] — the combined [`BillPouchBehaviour`] (`#[derive(NetworkBehaviour)]`).
//! - [`state`]     — in-memory [`NetworkState`] updated from incoming gossip messages.

pub mod behaviour;
pub mod fragment_gossip;
pub mod qos;
pub mod quality_monitor;
pub mod state;

pub use behaviour::{BillPouchBehaviour, FragmentRequest, FragmentResponse};
pub use fragment_gossip::{FragmentIndexAnnouncement, FragmentPointer, RemoteFragmentIndex};
pub use qos::{PeerQos, QosRegistry, FAULT_BLACKLISTED, FAULT_DEGRADED, FAULT_SUSPECTED};
pub use quality_monitor::run_quality_monitor;
pub use state::{NetworkState, NodeInfo};

use crate::{
    error::{BpError, BpResult},
    storage::StorageManager,
};
use futures::StreamExt;
use libp2p::{gossipsub, request_response, swarm::SwarmEvent, Multiaddr, PeerId, Swarm};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

/// Shared map of active Pouch `StorageManager`s, keyed by `service_id`.
pub type StorageManagerMap = Arc<RwLock<HashMap<String, Arc<RwLock<StorageManager>>>>>;

/// A single fragment that was pushed to a remote Pouch.
///
/// Stored in [`OutgoingAssignments`] so the quality monitor can later issue
/// Proof-of-Storage challenges for exactly the fragments each Pouch is
/// supposed to hold.
#[derive(Debug, Clone)]
pub struct OutgoingFragment {
    /// BLAKE3 prefix that identifies the chunk.
    pub chunk_id: String,
    /// UUID of the specific fragment within the chunk.
    pub fragment_id: String,
}

/// Map of remote Pouch peer IDs → fragments pushed to them.
///
/// Updated by the network loop whenever a `PushFragment` command is executed.
/// Read by [`run_quality_monitor`] to select challenge targets.
///
/// [`run_quality_monitor`]: crate::network::run_quality_monitor
pub type OutgoingAssignments = Arc<RwLock<HashMap<String, Vec<OutgoingFragment>>>>;

/// Commands the daemon can send to the network task over the [`mpsc`] channel.
///
/// The network loop runs inside a dedicated `tokio::spawn`ed task and receives
/// these commands via the `cmd_rx` half of the channel created in [`run_daemon`].
///
/// [`run_daemon`]: crate::daemon::run_daemon
#[derive(Debug)]
pub enum NetworkCommand {
    /// Subscribe to the gossipsub topic for `network_id`.
    JoinNetwork { network_id: String },
    /// Unsubscribe from the gossipsub topic for `network_id`.
    LeaveNetwork { network_id: String },
    /// Publish a serialised [`NodeInfo`] on the gossipsub topic for `network_id`.
    Announce {
        /// Network whose topic to publish on.
        network_id: String,
        /// Serialised [`NodeInfo`] bytes.
        payload: Vec<u8>,
    },
    /// Publish a serialised [`FragmentIndexAnnouncement`] on the index gossip
    /// topic (`billpouch/v1/{network_id}/index`) for `network_id`.
    AnnounceIndex {
        /// Network whose index topic to publish on.
        network_id: String,
        /// Serialised [`FragmentIndexAnnouncement`] bytes.
        payload: Vec<u8>,
    },
    /// Dial a remote peer at a known [`Multiaddr`].
    Dial { addr: Multiaddr },
    /// Push a fragment to a remote Pouch peer for storage.
    PushFragment {
        peer_id: PeerId,
        chunk_id: String,
        fragment_id: String,
        data: Vec<u8>,
    },
    /// Fetch all fragments a remote Pouch holds for a given chunk.
    /// The response is sent back through the oneshot channel.
    FetchChunkFragments {
        peer_id: PeerId,
        chunk_id: String,
        resp_tx: tokio::sync::oneshot::Sender<FragmentResponse>,
    },
    /// Ask the network loop to exit cleanly.
    Shutdown,
    /// Ping a remote peer for RTT measurement.
    ///
    /// The network loop sends a `FragmentRequest::Ping`, waits for the
    /// `FragmentResponse::Pong`, computes the RTT in milliseconds and
    /// forwards it to `resp_tx`.  On timeout or failure the sender is
    /// dropped without sending.
    Ping {
        peer_id: PeerId,
        /// Monotonic timestamp (ms) at send time — used to compute RTT.
        sent_at_ms: u64,
        /// Receives `rtt_ms` when the Pong arrives.
        resp_tx: tokio::sync::oneshot::Sender<u64>,
    },
    /// Issue a Proof-of-Storage challenge to a remote Pouch.
    ///
    /// The network loop sends `FragmentRequest::ProofOfStorage`, awaits a
    /// `FragmentResponse::ProofOfStorageOk`, and delivers `true` (proof
    /// received) or `false` (no response / fragment not found) to `resp_tx`.
    ProofOfStorage {
        peer_id: PeerId,
        chunk_id: String,
        fragment_id: String,
        nonce: u64,
        /// Receives `true` if a proof was returned, `false` otherwise.
        resp_tx: tokio::sync::oneshot::Sender<bool>,
    },
}

/// Construct a fully configured libp2p [`Swarm`] using the Tokio runtime.
///
/// The transport stack is: TCP → Noise (encryption) → Yamux (multiplexing).
/// The behaviour stack is: gossipsub + Kademlia + Identify + mDNS.
///
/// # Errors
/// Returns an error if any behaviour or transport component fails to initialise
/// (typically a key encoding issue or an OS-level socket error).
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
        .with_behaviour(|key| {
            BillPouchBehaviour::new(key)
                .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e.to_string()))
        })
        .map_err(|e| anyhow::anyhow!("{}", e))?
        .build();

    Ok(swarm)
}

/// Drive the P2P event loop until shutdown.
///
/// Spawned as a background task by [`run_daemon`].  Runs a `tokio::select!`
/// loop that concurrently:
/// - polls the libp2p swarm for events (gossip, mDNS, Identify, Kademlia),
/// - reads [`NetworkCommand`]s from `cmd_rx` and mutates swarm state.
///
/// The loop exits when the command channel is closed or a `Shutdown` command
/// is received.
///
/// # Arguments
/// - `swarm` — the libp2p swarm built by [`build_swarm`].
/// - `cmd_rx` — receiving end of the daemon → network command channel.
/// - `state` — shared [`NetworkState`] updated from gossip messages.
/// - `listen_addr` — multiaddr to bind the TCP listener on.
///
/// [`run_daemon`]: crate::daemon::run_daemon
pub async fn run_network_loop(
    mut swarm: Swarm<BillPouchBehaviour>,
    mut cmd_rx: mpsc::Receiver<NetworkCommand>,
    state: Arc<RwLock<NetworkState>>,
    listen_addr: Multiaddr,
    storage_managers: StorageManagerMap,
    outgoing_assignments: OutgoingAssignments,
    remote_fragment_index: Arc<RwLock<fragment_gossip::RemoteFragmentIndex>>,
) -> BpResult<()> {
    swarm
        .listen_on(listen_addr.clone())
        .map_err(|e| BpError::Network(e.to_string()))?;

    tracing::info!("Network listening on {}", listen_addr);

    let mut subscribed_networks: HashSet<String> = HashSet::new();

    // Map outbound request IDs → oneshot response channels for FetchChunkFragments.
    let mut pending_fetches: HashMap<
        request_response::OutboundRequestId,
        tokio::sync::oneshot::Sender<FragmentResponse>,
    > = HashMap::new();

    // Map outbound request IDs → (sent_at_ms, oneshot) for Ping RTT tracking.
    let mut pending_pings: HashMap<
        request_response::OutboundRequestId,
        (u64, tokio::sync::oneshot::Sender<u64>),
    > = HashMap::new();

    // Map outbound request IDs → oneshot for Proof-of-Storage challenges.
    let mut pending_pos: HashMap<
        request_response::OutboundRequestId,
        tokio::sync::oneshot::Sender<bool>,
    > = HashMap::new();

    loop {
        tokio::select! {
            // ── Incoming swarm event ──────────────────────────────────────────
            event = swarm.select_next_some() => {
                handle_swarm_event(event, &mut swarm, &state, &mut subscribed_networks, &storage_managers, &mut pending_fetches, &mut pending_pings, &mut pending_pos, &remote_fragment_index).await;
            }
            // ── Command from daemon ───────────────────────────────────────────
            cmd = cmd_rx.recv() => {
                match cmd {
                    None => {
                        tracing::info!("Network command channel closed — shutting down");
                        break;
                    }
                    Some(NetworkCommand::Shutdown) => {
                        tracing::info!("Network shutdown requested");
                        break;
                    }
                    Some(NetworkCommand::JoinNetwork { network_id }) => {
                        if !subscribed_networks.contains(&network_id) {
                            let topic = gossipsub::IdentTopic::new(NodeInfo::topic_name(&network_id));
                            if let Err(e) = swarm.behaviour_mut().gossipsub.subscribe(&topic) {
                                tracing::warn!("Failed to subscribe to {}: {}", network_id, e);
                            } else {
                                subscribed_networks.insert(network_id.clone());
                                tracing::info!("Joined network gossip: {}", network_id);
                            }
                            // Also subscribe to the fragment-index topic.
                            let idx_topic = gossipsub::IdentTopic::new(
                                fragment_gossip::FragmentIndexAnnouncement::topic_name(&network_id),
                            );
                            if let Err(e) = swarm.behaviour_mut().gossipsub.subscribe(&idx_topic) {
                                tracing::warn!("Failed to subscribe to index topic {}: {}", network_id, e);
                            }
                        }
                    }
                    Some(NetworkCommand::LeaveNetwork { network_id }) => {
                        let topic = gossipsub::IdentTopic::new(NodeInfo::topic_name(&network_id));
                        let _ = swarm.behaviour_mut().gossipsub.unsubscribe(&topic);
                        let idx_topic = gossipsub::IdentTopic::new(
                            fragment_gossip::FragmentIndexAnnouncement::topic_name(&network_id),
                        );
                        let _ = swarm.behaviour_mut().gossipsub.unsubscribe(&idx_topic);
                        subscribed_networks.remove(&network_id);
                    }
                    Some(NetworkCommand::Announce { network_id, payload }) => {
                        let topic = gossipsub::IdentTopic::new(NodeInfo::topic_name(&network_id));
                        if let Err(e) = swarm.behaviour_mut().gossipsub.publish(topic, payload) {
                            tracing::warn!("Gossip publish failed: {}", e);
                        }
                    }
                    Some(NetworkCommand::AnnounceIndex { network_id, payload }) => {
                        let topic = gossipsub::IdentTopic::new(
                            fragment_gossip::FragmentIndexAnnouncement::topic_name(&network_id),
                        );
                        if let Err(e) = swarm.behaviour_mut().gossipsub.publish(topic, payload) {
                            tracing::warn!("FragmentIndex gossip publish failed: {}", e);
                        }
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
                        // Record the assignment so the quality monitor can issue PoS challenges.
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
                    Some(NetworkCommand::Ping {
                        peer_id,
                        sent_at_ms,
                        resp_tx,
                    }) => {
                        let nonce = sent_at_ms; // reuse timestamp as nonce
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
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_swarm_event(
    event: SwarmEvent<behaviour::BillPouchBehaviourEvent>,
    swarm: &mut Swarm<BillPouchBehaviour>,
    state: &Arc<RwLock<NetworkState>>,
    _subscribed: &mut HashSet<String>,
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
) {
    match event {
        // ── Gossipsub: incoming NodeInfo announcement ─────────────────────
        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Gossipsub(
            gossipsub::Event::Message {
                propagation_source,
                message,
                ..
            },
        )) => match serde_json::from_slice::<NodeInfo>(&message.data) {
            Ok(node_info) => {
                tracing::debug!(
                    peer=%propagation_source,
                    fingerprint=%node_info.user_fingerprint,
                    svc=%node_info.service_type,
                    "Gossip NodeInfo received"
                );
                if let Ok(mut st) = state.write() {
                    st.upsert(node_info);
                }
            }
            Err(_) => {
                // Try as FragmentIndexAnnouncement.
                match serde_json::from_slice::<fragment_gossip::FragmentIndexAnnouncement>(
                    &message.data,
                ) {
                    Ok(ann) => {
                        tracing::debug!(
                            peer = %propagation_source,
                            chunk = %ann.chunk_id,
                            ptrs = ann.pointers.len(),
                            "Gossip FragmentIndex received"
                        );
                        if let Ok(mut idx) = remote_fragment_index.write() {
                            idx.upsert(ann);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to deserialize gossip message: {}", e);
                    }
                }
            }
        },

        // ── mDNS: discovered a local peer ─────────────────────────────────
        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Mdns(
            libp2p::mdns::Event::Discovered(list),
        )) => {
            for (peer_id, addr) in list {
                tracing::info!(peer=%peer_id, addr=%addr, "mDNS peer discovered");
                swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                swarm.behaviour_mut().kad.add_address(&peer_id, addr);
            }
        }

        // ── mDNS: peer expired ────────────────────────────────────────────
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

        // ── Identify: received remote peer info ───────────────────────────
        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::Identify(
            libp2p::identify::Event::Received { peer_id, info, .. },
        )) => {
            tracing::debug!(peer=%peer_id, agent=%info.agent_version, "Identify received");
            for addr in info.listen_addrs {
                swarm.behaviour_mut().kad.add_address(&peer_id, addr);
            }
        }

        // ── New listen address ────────────────────────────────────────────
        SwarmEvent::NewListenAddr { address, .. } => {
            tracing::info!(addr=%address, "Now listening");
        }

        // ── Connection established ────────────────────────────────────────
        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            tracing::debug!(peer=%peer_id, "Connection established");
        }

        // ── Connection closed ─────────────────────────────────────────────
        SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
            tracing::debug!(peer=%peer_id, cause=?cause, "Connection closed");
        }

        // ── Fragment exchange: incoming request ───────────────────────────
        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::FragmentExchange(
            request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Request {
                        request, channel, ..
                    },
            },
        )) => {
            let response = serve_fragment_request(&request, storage_managers);
            if let Err(e) = swarm
                .behaviour_mut()
                .fragment_exchange
                .send_response(channel, response)
            {
                tracing::warn!(peer=%peer, "send_response failed: {:?}", e);
            }
        }

        // ── Fragment exchange: incoming response ──────────────────────────
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
            // Route Pong responses to the waiting Ping oneshot channel.
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
            // Route ProofOfStorageOk to the waiting PoS oneshot channel.
            if let FragmentResponse::ProofOfStorageOk { .. } = &response {
                if let Some(tx) = pending_pos.remove(&request_id) {
                    tracing::debug!(peer = %peer, "ProofOfStorage: proof received");
                    let _ = tx.send(true);
                    return;
                }
            }
            // NotFound on a PoS challenge counts as a failure.
            if let FragmentResponse::NotFound = &response {
                if let Some(tx) = pending_pos.remove(&request_id) {
                    tracing::debug!(peer = %peer, "ProofOfStorage: fragment not found");
                    let _ = tx.send(false);
                    return;
                }
            }
            // Route fragment responses to the waiting fetch oneshot channel.
            if let Some(tx) = pending_fetches.remove(&request_id) {
                let _ = tx.send(response);
            } else {
                tracing::debug!(peer=%peer, "Untracked fragment response (fire-and-forget)");
            }
        }

        // ── Fragment exchange: outbound failure ─────────────────────────
        SwarmEvent::Behaviour(behaviour::BillPouchBehaviourEvent::FragmentExchange(
            request_response::Event::OutboundFailure {
                request_id, error, ..
            },
        )) => {
            tracing::warn!("Fragment request failed: {:?}", error);
            // Drop all channels so callers get notified via RecvError.
            pending_fetches.remove(&request_id);
            pending_pings.remove(&request_id);
            if let Some(tx) = pending_pos.remove(&request_id) {
                let _ = tx.send(false);
            }
        }

        _ => {}
    }
}

/// Serve a [`FragmentRequest`] from a remote peer by looking up local storage.
fn serve_fragment_request(
    req: &FragmentRequest,
    storage_managers: &StorageManagerMap,
) -> FragmentResponse {
    let managers = storage_managers.read().unwrap();
    match req {
        FragmentRequest::Fetch {
            chunk_id,
            fragment_id,
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
                            return FragmentResponse::Found {
                                data: fragment.to_bytes(),
                            }
                        }
                        Err(e) => tracing::warn!("serve_fragment load error: {e}"),
                    }
                }
            }
            FragmentResponse::NotFound
        }
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
            // Try to store in any manager that has capacity.
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
            // Load the fragment data and compute BLAKE3(data || nonce.to_le_bytes()).
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
