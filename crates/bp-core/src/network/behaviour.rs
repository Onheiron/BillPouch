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

/// Request sent between Pouch nodes for fragment operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FragmentRequest {
    /// Ask a remote Pouch for a specific fragment by id.
    Fetch {
        chunk_id: String,
        fragment_id: String,
    },
    /// Ask a remote Pouch for ALL fragments it holds for a chunk.
    FetchChunkFragments { chunk_id: String },
    /// Push a fragment to a remote Pouch for storage.
    Store {
        chunk_id: String,
        fragment_id: String,
        data: Vec<u8>,
    },
    /// Liveness ping — expects a `Pong` with the same nonce.
    Ping { nonce: u64 },
    /// Proof-of-Storage challenge — expects a BLAKE3 proof.
    ///
    /// The responder must load the fragment identified by `(chunk_id,
    /// fragment_id)`, compute `BLAKE3(raw_data || nonce.to_le_bytes())`,
    /// and return it in a [`FragmentResponse::ProofOfStorageOk`].
    ProofOfStorage {
        chunk_id: String,
        fragment_id: String,
        nonce: u64,
    },
}

/// Response to a fragment request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FragmentResponse {
    /// Single fragment found (response to Fetch).
    Found { data: Vec<u8> },
    /// Multiple fragments found (response to FetchChunkFragments).
    FoundMany { fragments: Vec<(String, Vec<u8>)> },
    /// Fragment/chunk not found on this peer.
    NotFound,
    /// Fragment stored successfully (response to Store).
    Stored,
    /// Store failed (response to Store).
    StoreFailed { reason: String },
    /// Pong response to a Ping — echoes the nonce.
    Pong { nonce: u64 },
    /// Proof-of-Storage response: `BLAKE3(fragment_data || nonce.to_le_bytes())`.
    ///
    /// Returned by a Pouch that successfully loaded the challenged fragment.
    ProofOfStorageOk { proof: [u8; 32] },
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
    pub fragment_exchange: request_response::cbor::Behaviour<FragmentRequest, FragmentResponse>,
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
                StreamProtocol::new("/billpouch/fragment/1.1.0"),
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
