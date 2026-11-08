use super::merkle;
use crate::error::{BpError, BpResult};
use libp2p::identity::{Keypair, PublicKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Held {
    Fragment {
        chunk_id: String,
        fragment_id: String,
    },
    Filler {
        blob_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commitment {
    pub held: Held,
    pub holder: String,
    pub root: [u8; 32],
    pub blocks: usize,
    pub size: u64,
    pub signer: String,
    pub signature: String,
}

impl Commitment {
    fn message(&self) -> Vec<u8> {
        let mut msg = b"billpouch/commitment/v1".to_vec();
        msg.extend_from_slice(
            &serde_json::to_vec(&(
                &self.held,
                &self.holder,
                self.root,
                self.blocks,
                self.size,
                &self.signer,
            ))
            .unwrap_or_default(),
        );
        msg
    }

    pub fn sign(
        device: &Keypair,
        signer: &str,
        held: Held,
        holder: &str,
        data: &[u8],
    ) -> BpResult<Self> {
        let mut commitment = Self {
            held,
            holder: holder.to_string(),
            root: merkle::root(data),
            blocks: merkle::block_count(data.len()),
            size: data.len() as u64,
            signer: signer.to_string(),
            signature: String::new(),
        };
        let signature = device
            .sign(&commitment.message())
            .map_err(|e| BpError::Identity(format!("cannot sign commitment: {e}")))?;
        commitment.signature = hex::encode(signature);
        Ok(commitment)
    }

    pub fn matches(&self, data: &[u8]) -> bool {
        self.size == data.len() as u64 && self.root == merkle::root(data)
    }

    pub fn is_signed_by(&self, key: &PublicKey) -> bool {
        hex::decode(&self.signature).is_ok_and(|s| key.verify(&self.message(), &s))
    }

    pub fn index_for(&self, seed: u64) -> usize {
        (seed % self.blocks.max(1) as u64) as usize
    }

    pub fn proves(&self, seed: u64, proof: &merkle::Proof) -> bool {
        proof.index == self.index_for(seed) && merkle::verify(&self.root, self.blocks, proof)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commitment_binds_data_holder_and_signer() {
        let device = Keypair::generate_ed25519();
        let other = Keypair::generate_ed25519();
        let data: Vec<u8> = (0..3 * merkle::BLOCK).map(|i| i as u8).collect();
        let held = Held::Filler {
            blob_id: "b".into(),
        };
        let c = Commitment::sign(&device, "me", held, "pouch", &data).unwrap();
        assert!(c.matches(&data));
        assert!(!c.matches(&data[1..]));
        assert!(c.is_signed_by(&device.public()));
        assert!(!c.is_signed_by(&other.public()));
        let mut moved = c.clone();
        moved.holder = "another pouch".into();
        assert!(!moved.is_signed_by(&device.public()));

        let seed = 7;
        let proof = merkle::prove(&data, c.index_for(seed)).unwrap();
        assert!(c.proves(seed, &proof));
        assert!(!c.proves(seed + 1, &proof));
    }
}
