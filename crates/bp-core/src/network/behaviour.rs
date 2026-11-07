use libp2p::{
    autonat, gossipsub, identify, identity::Keypair, kad, mdns, relay, request_response,
    swarm::NetworkBehaviour, PeerId, StreamProtocol,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FragmentRequest {
    FetchChunkFragments {
        chunk_id: String,
    },
    Store {
        chunk_id: String,
        fragment_id: String,
        data: Vec<u8>,
    },
    Ping {
        nonce: u64,
    },
    ProofOfStorage {
        chunk_id: String,
        fragment_id: String,
        nonce: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FragmentResponse {
    FoundMany { fragments: Vec<(String, Vec<u8>)> },
    NotFound,
    Stored,
    StoreFailed { reason: String },
    Pong { nonce: u64 },
    ProofOfStorageOk { proof: [u8; 32] },
}

#[derive(NetworkBehaviour)]
pub struct BillPouchBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub kad: kad::Behaviour<kad::store::MemoryStore>,
    pub identify: identify::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub fragment_exchange: request_response::cbor::Behaviour<FragmentRequest, FragmentResponse>,
    pub autonat: autonat::Behaviour,
    pub relay: relay::client::Behaviour,
}

impl BillPouchBehaviour {
    pub fn new(keypair: &Keypair, relay_client: relay::client::Behaviour) -> anyhow::Result<Self> {
        let peer_id = PeerId::from_public_key(&keypair.public());

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

        let store = kad::store::MemoryStore::new(peer_id);
        let kad = kad::Behaviour::new(peer_id, store);

        let identify = identify::Behaviour::new(identify::Config::new(
            "/billpouch/id/1.0.0".into(),
            keypair.public(),
        ));

        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id)?;

        let fragment_exchange = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/billpouch/fragment/1.1.0"),
                request_response::ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );

        let autonat = autonat::Behaviour::new(peer_id, autonat::Config::default());

        Ok(Self {
            gossipsub,
            kad,
            identify,
            mdns,
            fragment_exchange,
            autonat,
            relay: relay_client,
        })
    }
}
