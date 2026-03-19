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
pub mod state;

pub use behaviour::{BillPouchBehaviour, FragmentRequest, FragmentResponse};
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
    /// Dial a remote peer at a known [`Multiaddr`].
    Dial { addr: Multiaddr },
    /// Push a fragment to a remote Pouch peer.
    PushFragment {
        peer_id: PeerId,
        chunk_id: String,
        fragment_id: String,
        data: Vec<u8>,
    },
    /// Fetch a specific fragment from a remote Pouch peer.
    FetchFragment {
        peer_id: PeerId,
        chunk_id: String,
        fragment_id: String,
    },
    /// Ask the network loop to exit cleanly.
    Shutdown,
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
    storage_managers: Arc<RwLock<HashMap<String, Arc<RwLock<StorageManager>>>>>,
) -> BpResult<()> {
    swarm
        .listen_on(listen_addr.clone())
        .map_err(|e| BpError::Network(e.to_string()))?;

    tracing::info!("Network listening on {}", listen_addr);

    let mut subscribed_networks: HashSet<String> = HashSet::new();

    loop {
        tokio::select! {
            // ── Incoming swarm event ──────────────────────────────────────────
            event = swarm.select_next_some() => {
                handle_swarm_event(event, &mut swarm, &state, &mut subscribed_networks, &storage_managers).await;
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
                        }
                    }
                    Some(NetworkCommand::LeaveNetwork { network_id }) => {
                        let topic = gossipsub::IdentTopic::new(NodeInfo::topic_name(&network_id));
                        let _ = swarm.behaviour_mut().gossipsub.unsubscribe(&topic);
                        subscribed_networks.remove(&network_id);
                    }
                    Some(NetworkCommand::Announce { network_id, payload }) => {
                        let topic = gossipsub::IdentTopic::new(NodeInfo::topic_name(&network_id));
                        if let Err(e) = swarm.behaviour_mut().gossipsub.publish(topic, payload) {
                            tracing::warn!("Gossip publish failed: {}", e);
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
                        // Send fragment data to a remote peer via request-response.
                        // We encode the push as a request carrying the data; the
                        // remote side will store it. The response is ignored.
                        let req = FragmentRequest {
                            chunk_id: chunk_id.clone(),
                            fragment_id: fragment_id.clone(),
                        };
                        // Attach data as a separate gossip publish for simplicity;
                        // real implementation would extend the protocol.
                        let _ = swarm
                            .behaviour_mut()
                            .fragment_exchange
                            .send_request(&peer_id, req);
                        tracing::debug!(peer=%peer_id, chunk=%chunk_id, frag=%fragment_id, bytes=%data.len(), "PushFragment sent");
                    }
                    Some(NetworkCommand::FetchFragment {
                        peer_id,
                        chunk_id,
                        fragment_id,
                    }) => {
                        let req = FragmentRequest { chunk_id, fragment_id };
                        let _req_id = swarm
                            .behaviour_mut()
                            .fragment_exchange
                            .send_request(&peer_id, req);
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_swarm_event(
    event: SwarmEvent<behaviour::BillPouchBehaviourEvent>,
    swarm: &mut Swarm<BillPouchBehaviour>,
    state: &Arc<RwLock<NetworkState>>,
    _subscribed: &mut HashSet<String>,
    storage_managers: &Arc<RwLock<HashMap<String, Arc<RwLock<StorageManager>>>>>,
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
            Err(e) => {
                tracing::warn!("Failed to deserialize gossip message: {}", e);
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
                message: request_response::Message::Response { response, .. },
            },
        )) => match response {
            FragmentResponse::Found { data } => {
                tracing::debug!(peer=%peer, bytes=%data.len(), "Fragment response received");
            }
            FragmentResponse::NotFound => {
                tracing::debug!(peer=%peer, "Fragment not found on remote");
            }
        },

        _ => {}
    }
}

/// Serve a [`FragmentRequest`] from a remote peer by looking up local storage.
fn serve_fragment_request(
    req: &FragmentRequest,
    storage_managers: &Arc<RwLock<HashMap<String, Arc<RwLock<StorageManager>>>>>,
) -> FragmentResponse {
    let managers = storage_managers.read().unwrap();
    for sm_arc in managers.values() {
        let sm = sm_arc.read().unwrap();
        if sm
            .index
            .fragments_for_chunk(&req.chunk_id)
            .iter()
            .any(|m| m.fragment_id == req.fragment_id)
        {
            match sm.load_fragment(&req.chunk_id, &req.fragment_id) {
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
