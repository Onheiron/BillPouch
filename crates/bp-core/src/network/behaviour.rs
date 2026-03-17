//! Combined libp2p `NetworkBehaviour` for BillPouch.
//!
//! Stack:
//!   • Gossipsub  — flooding broadcast for NodeInfo announcements
//!   • Kademlia   — DHT for peer discovery and content addressing
//!   • Identify   — exchange protocol version and listen addresses
//!   • mDNS       — local-network peer discovery (zero-config)

use libp2p::{
    gossipsub, identify, kad,
    mdns,
    swarm::NetworkBehaviour,
    identity::Keypair,
    PeerId,
};
use std::time::Duration;

/// Single combined behaviour injected into the libp2p Swarm.
#[derive(NetworkBehaviour)]
pub struct BillPouchBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub kad: kad::Behaviour<kad::store::MemoryStore>,
    pub identify: identify::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
}

impl BillPouchBehaviour {
    /// Build the combined behaviour from a keypair (called inside SwarmBuilder).
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

        Ok(Self { gossipsub, kad, identify, mdns })
    }
}
