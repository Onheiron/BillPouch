pub mod behaviour;
pub mod state;

pub use behaviour::BillPouchBehaviour;
pub use state::{NetworkState, NodeInfo};

use crate::error::{BpError, BpResult};
use futures::StreamExt;
use libp2p::{gossipsub, swarm::SwarmEvent, Multiaddr, Swarm};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

/// Commands the daemon can send to the network task.
#[derive(Debug)]
pub enum NetworkCommand {
    /// Subscribe to a network's gossip topic.
    JoinNetwork { network_id: String },
    /// Unsubscribe from a network's gossip topic.
    LeaveNetwork { network_id: String },
    /// Broadcast a NodeInfo announcement on a network topic.
    Announce {
        network_id: String,
        payload: Vec<u8>,
    },
    /// Dial a remote peer.
    Dial { addr: Multiaddr },
    /// Graceful shutdown.
    Shutdown,
}

/// Build the libp2p Swarm using tokio transport.
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

/// Run the P2P network loop.
///
/// Handles incoming swarm events (gossip messages, kad events, mDNS discovery)
/// and outgoing commands from the daemon task.
pub async fn run_network_loop(
    mut swarm: Swarm<BillPouchBehaviour>,
    mut cmd_rx: mpsc::Receiver<NetworkCommand>,
    state: Arc<RwLock<NetworkState>>,
    listen_addr: Multiaddr,
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
                handle_swarm_event(event, &mut swarm, &state, &mut subscribed_networks).await;
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

        _ => {}
    }
}
