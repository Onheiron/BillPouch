use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p::{
    autonat, gossipsub, identify, identity::Keypair, kad, relay, request_response,
    swarm::NetworkBehaviour, PeerId, StreamProtocol,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FragmentRequest {
    FetchChunkFragments {
        chunk_id: String,
        grant: crate::files::Grant,
    },
    Store {
        chunk_id: String,
        fragment_id: String,
        data: Vec<u8>,
        grant: crate::files::Grant,
        layer: crate::storage::StorageTier,
        holders: usize,
        commitment: crate::storage::commitment::Commitment,
    },
    Ping {
        nonce: u64,
    },
    ProofOfStorage {
        target: crate::storage::PosTarget,
        seed: u64,
    },
    Fill {
        network: String,
        blob_id: String,
        data: Vec<u8>,
        commitment: crate::storage::commitment::Commitment,
    },
    Commits {
        network: String,
        from: u64,
        cert: crate::identity::DeviceCert,
    },
    Catalog {
        network: String,
        layer: crate::storage::StorageTier,
    },
    Recode {
        chunk_id: String,
        owner: crate::storage::ChunkOwner,
        route: Vec<String>,
        partial: Option<Vec<u8>>,
    },
    Repaired {
        chunk_id: String,
        owner: crate::storage::ChunkOwner,
        fragment_id: String,
        fragment: Vec<u8>,
        commitment: crate::storage::commitment::Commitment,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FragmentResponse {
    FoundMany {
        fragments: Vec<(String, Vec<u8>)>,
    },
    NotFound,
    Stored,
    StoreFailed {
        reason: String,
    },
    Pong {
        nonce: u64,
    },
    ProofOfStorageOk {
        proof: crate::storage::merkle::Proof,
        commitment: Box<crate::storage::commitment::Commitment>,
    },
    Commits {
        commits: Vec<crate::consensus::Commit>,
    },
    Catalog {
        chunks: Vec<crate::storage::CatalogEntry>,
    },
}

pub const REQUEST_SIZE_MAXIMUM: u64 = 16 * 1024 * 1024;
pub const RESPONSE_SIZE_MAXIMUM: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct FragmentCodec;

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> std::io::Result<T> {
    cbor4ii::serde::from_slice(bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
}

fn encode<T: Serialize>(value: &T) -> std::io::Result<Vec<u8>> {
    cbor4ii::serde::to_vec(Vec::new(), value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
}

impl request_response::Codec for FragmentCodec {
    type Protocol = StreamProtocol;
    type Request = FragmentRequest;
    type Response = FragmentResponse;

    async fn read_request<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
    ) -> std::io::Result<FragmentRequest>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut bytes = Vec::new();
        io.take(REQUEST_SIZE_MAXIMUM)
            .read_to_end(&mut bytes)
            .await?;
        decode(&bytes)
    }

    async fn read_response<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
    ) -> std::io::Result<FragmentResponse>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut bytes = Vec::new();
        io.take(RESPONSE_SIZE_MAXIMUM)
            .read_to_end(&mut bytes)
            .await?;
        decode(&bytes)
    }

    async fn write_request<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
        request: FragmentRequest,
    ) -> std::io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        io.write_all(&encode(&request)?).await
    }

    async fn write_response<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
        response: FragmentResponse,
    ) -> std::io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        io.write_all(&encode(&response)?).await
    }
}

#[derive(NetworkBehaviour)]
pub struct BillPouchBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub kad: kad::Behaviour<kad::store::MemoryStore>,
    pub identify: identify::Behaviour,
    pub fragment_exchange: request_response::Behaviour<FragmentCodec>,
    pub autonat: autonat::Behaviour,
    pub relay: relay::client::Behaviour,
}

impl BillPouchBehaviour {
    pub fn new(keypair: &Keypair, relay_client: relay::client::Behaviour) -> anyhow::Result<Self> {
        let peer_id = PeerId::from_public_key(&keypair.public());

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10))
            .check_explicit_peers_ticks(1)
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

        let fragment_exchange = request_response::Behaviour::with_codec(
            FragmentCodec,
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
            fragment_exchange,
            autonat,
            relay: relay_client,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_larger_than_a_mebibyte_round_trip() {
        let data = vec![7; 2 * 1024 * 1024];
        let commitment = crate::storage::commitment::Commitment::sign(
            &libp2p::identity::Keypair::generate_ed25519(),
            "sponsor",
            crate::storage::commitment::Held::Filler {
                blob_id: "b".into(),
            },
            "pouch",
            &data,
        )
        .unwrap();
        let request = FragmentRequest::Fill {
            network: "amici".into(),
            blob_id: "b".into(),
            data,
            commitment,
        };
        let bytes = encode(&request).unwrap();
        assert!((bytes.len() as u64) < REQUEST_SIZE_MAXIMUM);
        match decode::<FragmentRequest>(&bytes).unwrap() {
            FragmentRequest::Fill { data, .. } => assert_eq!(data.len(), 2 * 1024 * 1024),
            other => panic!("unexpected {other:?}"),
        }
    }
}
