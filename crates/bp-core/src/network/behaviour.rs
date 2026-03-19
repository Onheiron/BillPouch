//! Combined libp2p `NetworkBehaviour` for BillPouch.
//!
//! Stack:
//!   • Gossipsub        — flooding broadcast for NodeInfo announcements
//!   • Kademlia         — DHT for peer discovery and content addressing
//!   • Identify         — exchange protocol version and listen addresses
//!   • mDNS             — local-network peer discovery (zero-config)
//!   • RequestResponse  — direct fragment fetch/push between Pouches

use libp2p::{
    gossipsub, identify, identity::Keypair, kad, mdns, request_response, swarm::NetworkBehaviour,
    PeerId, StreamProtocol,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ── Fragment exchange protocol ────────────────────────────────────────────────

/// Request sent from a Bill (or Pouch) to a remote Pouch asking for one
/// specific fragment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentRequest {
    /// BLAKE3 chunk hash prefix of the target chunk.
    pub chunk_id: String,
    /// UUID of the specific fragment to retrieve.
    pub fragment_id: String,
}

/// Response carrying the raw `.frag` blob, or a not-found signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FragmentResponse {
    /// Fragment bytes in the `EncodedFragment` binary format.
    Found { data: Vec<u8> },
    /// This peer does not hold the requested fragment.
    NotFound,
}

// ── Combined behaviour ────────────────────────────────────────────────────────

/// Single combined behaviour injected into the libp2p Swarm.
///
/// All five sub-behaviours are polled by the swarm in a single `select!`-style
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
    /// Direct fragment fetch/push between Pouch nodes.
    pub fragment_exchange:
        request_response::cbor::Behaviour<FragmentRequest, FragmentResponse>,
}

impl BillPouchBehaviour {
    /// Build the combined behaviour from a keypair (called inside `SwarmBuilder`).
    ///
    /// Configures:
    /// - Gossipsub with strict message signing and a 10-second heartbeat.
    /// - Kademlia with an in-memory record store.
    /// - Identify with the `/billpouch/id/1.0.0` protocol string.
    /// - mDNS with default settings.
    /// - RequestResponse with the `/billpouch/fragment/1.0.0` protocol.
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

        // ── Fragment exchange ──────────────────────────────────────────────────
        let fragment_exchange = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/billpouch/fragment/1.0.0"),
                request_response::ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );

        Ok(Self {
            gossipsub,
            kad,
            identify,
            mdns,
            fragment_exchange,
        })
    }
}
