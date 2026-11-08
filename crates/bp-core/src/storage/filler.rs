use super::merkle::BLOCK;
use rand::{RngCore, SeedableRng};

pub const BLOB_BLOCKS: usize = 256;
pub const BLOB_BYTES: usize = BLOB_BLOCKS * BLOCK;

pub fn blob() -> (String, Vec<u8>) {
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let mut data = vec![0u8; BLOB_BYTES];
    rand::rngs::StdRng::from_seed(seed).fill_bytes(&mut data);
    (hex::encode(&blake3::hash(&seed).as_bytes()[..8]), data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blobs_are_full_size_and_never_repeat() {
        let (a, data_a) = blob();
        let (b, data_b) = blob();
        assert_eq!(data_a.len(), BLOB_BYTES);
        assert_ne!(a, b);
        assert_ne!(data_a, data_b);
    }
}
