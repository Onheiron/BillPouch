use super::merkle::{self, BLOCK};
use crate::config;
use rand::{RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const BLOB_BLOCKS: usize = 256;
pub const BLOB_BYTES: usize = BLOB_BLOCKS * BLOCK;

pub fn block(seed: &[u8; 32], index: usize) -> Vec<u8> {
    let block_seed = blake3::keyed_hash(seed, &(index as u64).to_le_bytes());
    let mut rng = rand::rngs::StdRng::from_seed(*block_seed.as_bytes());
    let mut out = vec![0u8; BLOCK];
    rng.fill_bytes(&mut out);
    out
}

pub fn blob(seed: &[u8; 32]) -> Vec<u8> {
    (0..BLOB_BLOCKS).flat_map(|i| block(seed, i)).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sponsored {
    pub blob_id: String,
    pub network: String,
    pub seed: [u8; 32],
    pub root: [u8; 32],
}

impl Sponsored {
    pub fn new(network: &str) -> (Self, Vec<u8>) {
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        let data = blob(&seed);
        let sponsored = Self {
            blob_id: hex::encode(&blake3::hash(&seed).as_bytes()[..8]),
            network: network.to_string(),
            seed,
            root: merkle::root(&data),
        };
        (sponsored, data)
    }

    pub fn verify(&self, proof: &merkle::Proof) -> bool {
        merkle::verify(&self.root, BLOB_BLOCKS, proof)
            && proof.block == block(&self.seed, proof.index)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FillerBook {
    pub by_pouch: BTreeMap<String, Vec<Sponsored>>,
    #[serde(skip)]
    persistent: bool,
}

impl FillerBook {
    pub fn load() -> Self {
        let mut book: Self = config::fillers_path()
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        book.persistent = true;
        book
    }

    pub fn save(&self) {
        if !self.persistent {
            return;
        }
        let result = config::fillers_path().and_then(|p| {
            std::fs::write(p, serde_json::to_vec(self)?)?;
            Ok(())
        });
        if let Err(e) = result {
            tracing::warn!("cannot save the filler book: {e}");
        }
    }

    pub fn add(&mut self, pouch: &str, sponsored: Sponsored) {
        self.by_pouch
            .entry(pouch.to_string())
            .or_default()
            .push(sponsored);
        self.save();
    }

    pub fn forget(&mut self, pouch: &str, blobs: &[String]) {
        if let Some(list) = self.by_pouch.get_mut(pouch) {
            list.retain(|s| !blobs.contains(&s.blob_id));
        }
        self.save();
    }

    pub fn bytes_for(&self, pouch: &str) -> u64 {
        self.by_pouch
            .get(pouch)
            .map_or(0, |l| (l.len() * BLOB_BYTES) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_blob_passes_and_anything_else_fails() {
        let (sponsored, data) = Sponsored::new("amici");
        assert_eq!(data.len(), BLOB_BYTES);
        for index in [0, 17, BLOB_BLOCKS - 1] {
            let proof = merkle::prove(&data, index).unwrap();
            assert!(sponsored.verify(&proof));
        }
        let (other, other_data) = Sponsored::new("amici");
        assert!(!sponsored.verify(&merkle::prove(&other_data, 3).unwrap()));
        assert!(!other.verify(&merkle::prove(&data, 3).unwrap()));
    }

    #[test]
    fn blocks_are_reproducible_from_the_seed_alone() {
        let (sponsored, data) = Sponsored::new("amici");
        assert_eq!(block(&sponsored.seed, 5), data[5 * BLOCK..6 * BLOCK]);
        assert_ne!(block(&sponsored.seed, 5), block(&sponsored.seed, 6));
    }

    #[test]
    fn the_book_tracks_and_forgets_blobs_per_pouch() {
        let mut book = FillerBook::default();
        let (a, _) = Sponsored::new("amici");
        let (b, _) = Sponsored::new("amici");
        book.add("p1", a.clone());
        book.add("p1", b);
        assert_eq!(book.bytes_for("p1"), 2 * BLOB_BYTES as u64);
        book.forget("p1", &[a.blob_id]);
        assert_eq!(book.bytes_for("p1"), BLOB_BYTES as u64);
        assert_eq!(book.bytes_for("p2"), 0);
    }
}
