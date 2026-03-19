//! Combined libp2p `NetworkBehaviour` for BillPouch.
//!
//! Stack:
//!   • Gossipsub  — flooding broadcast for NodeInfo announcements
//!   • Kademlia   — DHT for peer discovery and content addressing
//!   • Identify   — exchange protocol version and listen addresses
//!   • mDNS       — local-network peer discovery (zero-config)

use libp2p::{gossipsub, identify, identity::Keypair, kad, mdns, swarm::NetworkBehaviour, PeerId};
use std::time::Duration;

/// Single combined behaviour injected into the libp2p Swarm.
///
/// All four sub-behaviours are polled by the swarm in a single `select!`-style
/// event loop inside [`run_network_loop`].
///
/// [`run_network_loop`]: crate::network::run_network_loop
#[derive(NetworkBehaviour)]
pub struct BillPouchBehaviour {
    /// Flooding pub/sub: used to broadcast [`NodeInfo`] announcements.
    ///
    /// [`NodeInfo`]: crate::network::state::NodeInfo
    pub gossipsub: gossipsub::Behaviour,
    /// Kademlia DHT: distributed peer discovery and content addressing.
    pub kad: kad::Behaviour<kad::store::MemoryStore>,
    /// Identify: exchange protocol version string and listen addresses with peers.
    pub identify: identify::Behaviour,
    /// mDNS: zero-configuration local-network peer discovery via multicast DNS.
    pub mdns: mdns::tokio::Behaviour,
}

impl BillPouchBehaviour {
    /// Build the combined behaviour from a keypair (called inside `SwarmBuilder`).
    ///
    /// Configures:
    /// - Gossipsub with strict message signing and a 10-second heartbeat.
    /// - Kademlia with an in-memory record store.
    /// - Identify with the `/billpouch/id/1.0.0` protocol string.
    /// - mDNS with default settings.
    ///
    /// # Errors
    /// Returns an error if gossipsub config validation fails or mDNS cannot
    /// bind its multicast socket.
    pub fn new(keypair: &Keypair) -> anyhow::Result<Self> {
        let peer_id = PeerId::from_public_key(&keypair.public());

        // ── Gossipsub ─────────────────────────────────────────────────────────
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10))
            .validation_mode(gossipsub::ValidationMode::Strict)
            .history_gossip(5)
            .build()
            .map_err(|e| anyhow::anyhow!("Gossipsub config error: {}", e))?;

        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(keypair.clone()),
            gossipsub_config,
        )
        .map_err(|e| anyhow::anyhow!("Gossipsub init error: {}", e))?;

        // ── Kademlia ──────────────────────────────────────────────────────────
        let store = kad::store::MemoryStore::new(peer_id);
        let kad = kad::Behaviour::new(peer_id, store);

        // ── Identify ──────────────────────────────────────────────────────────
        let identify = identify::Behaviour::new(identify::Config::new(
            "/billpouch/id/1.0.0".into(),
            keypair.public(),
        ));

        // ── mDNS ──────────────────────────────────────────────────────────────
        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id)?;

        Ok(Self {
            gossipsub,
            kad,
            identify,
            mdns,
        })
    }
}
