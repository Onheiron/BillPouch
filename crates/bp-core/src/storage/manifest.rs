//! File manifest — per-file metadata for the BillPouch distributed FS.
//!
//! ## Overview
//!
//! Every file uploaded to a BillPouch network is described by a [`FileManifest`].
//! The manifest is stored on the network (gossipped or fetched on demand) and
//! describes:
//!
//! - The chunking and coding parameters (`k`, `n`, `q`, `ph`, `pe`).
//! - Where each fragment lives (which Pouch peer holds it).
//! - File metadata (name, size) encrypted with the **network metadata key**.
//!
//! ## Network metadata key
//!
//! Each network derives a shared 32-byte symmetric key from its `network_id`:
//!
//! ```text
//! network_meta_key = BLAKE3("billpouch/meta/v1" || network_id)
//! ```
//!
//! All nodes in the network can compute this key autonomously, but it is not
//! user-controlled or user-visible — the daemon uses it internally to encrypt
//! and decrypt file names and other identifying metadata.
//!
//! > **v0.1 note:** The key is deterministically derived from `network_id`.
//! > Future versions will use a Diffie-Hellman group keying scheme for stronger
//! > isolation between networks.
//!
//! ## Encryption scheme
//!
//! Metadata is encrypted with a BLAKE3-based authenticated stream cipher:
//!
//! ```text
//! nonce    = random 16 bytes
//! keystream = concat(BLAKE3_keyed(key, nonce || i) for i = 0, 1, 2, …)
//!             truncated to |plaintext| bytes
//! ciphertext = plaintext XOR keystream
//! mac        = BLAKE3_keyed(key, nonce || ciphertext)   (32 bytes)
//! output     = nonce(16) || mac(32) || ciphertext
//! ```
//!
//! ## File upload pipeline
//!
//! ```text
//! File (user key)
//!   │
//!   ▼ 1. Chunking  (chunk_size bytes each)
//!   │
//!   ▼ 2. Encrypt each chunk  (user's symmetric key, AES-256-GCM / ChaCha20)
//!   │
//!   ▼ 3. RLNC encode   k → n fragments per chunk
//!   │      k = compute_coding_params(stabilities, ph, q_target).k
//!   │
//!   ▼ 4. Distribute one fragment per Pouch peer
//!         (no local copy; originating Bill node has no storage)
//! ```
//!
//! ## File retrieval pipeline
//!
//! ```text
//! Request propagates via gossip (tree expansion)
//!   │
//!   ▼  per chunk: collect ≥ k fragments from Pouch peers
//!   ▼  RLNC decode  → encrypted chunk
//!   ▼  decrypt with user's key → plaintext chunk
//!   ▼  reassemble chunks → file
//! ```

use crate::error::{BpError, BpResult};
use rand::RngCore;
use serde::{Deserialize, Serialize};

// ── Network metadata key ──────────────────────────────────────────────────────

/// 32-byte symmetric key shared by all nodes in a given network.
///
/// Used exclusively to encrypt/decrypt file metadata (names, …) so that the
/// network can manage files autonomously without user involvement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkMetaKey(pub [u8; 32]);

impl NetworkMetaKey {
    /// Derive the network metadata key from a `network_id` string.
    ///
    /// ```text
    /// key = BLAKE3("billpouch/meta/v1" || network_id)
    /// ```
    pub fn for_network(network_id: &str) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(b"billpouch/meta/v1");
        h.update(network_id.as_bytes());
        let hash = h.finalize();
        Self(*hash.as_bytes())
    }

    /// Encrypt `plaintext` and return an authenticated ciphertext blob.
    ///
    /// Format: `nonce(16) || mac(32) || ciphertext`.
    pub fn encrypt(&self, plaintext: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce);

        let keystream = self.keystream(&nonce, plaintext.len());
        let ciphertext: Vec<u8> = plaintext
            .iter()
            .zip(keystream.iter())
            .map(|(a, b)| a ^ b)
            .collect();

        let mac = self.mac(&nonce, &ciphertext);

        let mut out = Vec::with_capacity(16 + 32 + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&mac);
        out.extend_from_slice(&ciphertext);
        out
    }

    /// Decrypt and authenticate a blob produced by [`encrypt`](Self::encrypt).
    ///
    /// Returns the plaintext or an error if the MAC is invalid or the blob is
    /// too short.
    pub fn decrypt(&self, blob: &[u8]) -> BpResult<Vec<u8>> {
        if blob.len() < 48 {
            return Err(BpError::Coding(
                "Encrypted metadata blob is too short".into(),
            ));
        }
        let nonce: [u8; 16] = blob[..16].try_into().unwrap();
        let stored_mac: [u8; 32] = blob[16..48].try_into().unwrap();
        let ciphertext = &blob[48..];

        let expected_mac = self.mac(&nonce, ciphertext);
        if expected_mac != stored_mac {
            return Err(BpError::Coding(
                "Metadata MAC verification failed".into(),
            ));
        }

        let keystream = self.keystream(&nonce, ciphertext.len());
        let plaintext: Vec<u8> = ciphertext
            .iter()
            .zip(keystream.iter())
            .map(|(a, b)| a ^ b)
            .collect();

        Ok(plaintext)
    }

    // ── Internals ─────────────────────────────────────────────────────────

    /// Generate a keystream of `length` bytes using BLAKE3 in counter mode.
    fn keystream(&self, nonce: &[u8; 16], length: usize) -> Vec<u8> {
        let mut ks = Vec::with_capacity(length + 32);
        let mut counter = 0u64;
        while ks.len() < length {
            let mut h = blake3::Hasher::new_keyed(&self.0);
            h.update(b"ks");
            h.update(nonce);
            h.update(&counter.to_le_bytes());
            ks.extend_from_slice(h.finalize().as_bytes());
            counter += 1;
        }
        ks.truncate(length);
        ks
    }

    /// Compute a BLAKE3-keyed MAC over `nonce || ciphertext`.
    fn mac(&self, nonce: &[u8; 16], ciphertext: &[u8]) -> [u8; 32] {
        let mut h = blake3::Hasher::new_keyed(&self.0);
        h.update(b"mac");
        h.update(nonce);
        h.update(ciphertext);
        *h.finalize().as_bytes()
    }
}

// ── Fragment location ─────────────────────────────────────────────────────────

/// Location of a single RLNC fragment in the network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FragmentLocation {
    /// UUID of the fragment (matches `EncodedFragment::id`).
    pub fragment_id: String,
    /// libp2p PeerId (base58) of the Pouch that stores this fragment.
    pub pouch_peer_id: String,
    /// GF(2⁸) coding vector stored alongside the data.
    /// Length == `k`; needed by the challenger for Proof-of-Storage verification.
    pub coding_vector: Vec<u8>,
}

// ── Chunk manifest ────────────────────────────────────────────────────────────

/// Manifest entry for a single chunk of a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkManifest {
    /// BLAKE3 hash prefix (16 hex chars) of the **plaintext** chunk.
    pub chunk_id: String,
    /// Zero-based index of this chunk in the file.
    pub chunk_index: usize,
    /// Original byte count of this chunk (before padding and encryption).
    /// Used to trim padding after decoding.
    pub original_size: usize,
    /// Where each of the `n` fragments lives in the network.
    pub fragment_locations: Vec<FragmentLocation>,
}

impl ChunkManifest {
    /// Fragment locations on a specific peer (for fragment-fetch routing).
    pub fn fragments_on_peer<'a>(&'a self, peer_id: &str) -> Vec<&'a FragmentLocation> {
        self.fragment_locations
            .iter()
            .filter(|loc| loc.pouch_peer_id == peer_id)
            .collect()
    }

    /// All Pouch peer IDs that hold at least one fragment of this chunk.
    pub fn holder_peers(&self) -> Vec<&str> {
        self.fragment_locations
            .iter()
            .map(|loc| loc.pouch_peer_id.as_str())
            .collect()
    }
}

// ── File manifest ─────────────────────────────────────────────────────────────

/// Complete metadata descriptor for a file stored in a BillPouch network.
///
/// ## Field visibility
///
/// | Field           | Who can read                 | Why                              |
/// |-----------------|------------------------------|----------------------------------|
/// | `file_id`       | All network nodes            | Indexing and routing             |
/// | `owner_fingerprint` | All                      | Attribution                      |
/// | `encrypted_name`| Only nodes with network key  | Private metadata                 |
/// | `size_bytes`    | All                          | Storage planning                 |
/// | `k`, `n`, `q`  | All                          | Codec operation                  |
/// | `ph`, `pe`      | All                          | Health monitoring                |
/// | `chunks`        | All                          | Fragment routing                 |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileManifest {
    /// UUID v4 identifying this file in the network.
    pub file_id: String,

    /// SHA-256(pubkey)[0..8] hex of the uploading user.
    pub owner_fingerprint: String,

    /// Network this file belongs to.
    pub network_id: String,

    /// Filename encrypted with the network metadata key.
    ///
    /// Use [`NetworkMetaKey::encrypt`] / [`NetworkMetaKey::decrypt`] with the
    /// key from [`NetworkMetaKey::for_network`] to read or write.
    pub encrypted_name: Vec<u8>,

    /// Original file size in bytes.
    pub size_bytes: u64,

    /// Chunk size in bytes used during upload (e.g. 1 MiB = 1_048_576).
    pub chunk_size: usize,

    /// Total number of chunks.
    pub num_chunks: usize,

    /// Recovery threshold: minimum fragments needed to reconstruct any chunk.
    ///
    /// Computed at upload time via
    /// [`crate::coding::params::compute_coding_params`].
    pub k: usize,

    /// Total fragments generated per chunk (= k + redundancy).
    pub n: usize,

    /// Effective redundancy overhead fraction: `(n − k) / k`.
    pub q: f64,

    /// Target recovery probability declared at upload time (e.g. `0.999`).
    pub ph: f64,

    /// **Rolling** effective recovery probability, updated by the daemon as
    /// QoS measurements evolve.  Starts equal to [`ph`](Self::ph) at upload.
    pub pe: f64,

    /// Per-chunk location manifests (one entry per chunk).
    pub chunks: Vec<ChunkManifest>,

    /// Unix timestamp (seconds) when the file was uploaded.
    pub created_at: u64,
}

impl FileManifest {
    /// Decrypt and return the original filename.
    ///
    /// # Errors
    /// Returns `Err` if the key is wrong or the blob is corrupted.
    pub fn decrypt_name(&self, key: &NetworkMetaKey) -> BpResult<String> {
        let bytes = key.decrypt(&self.encrypted_name)?;
        String::from_utf8(bytes)
            .map_err(|e| BpError::Coding(format!("Filename is not valid UTF-8: {e}")))
    }

    /// Set the filename by encrypting it with the given network key.
    pub fn set_name(&mut self, filename: &str, key: &NetworkMetaKey) {
        self.encrypted_name = key.encrypt(filename.as_bytes());
    }

    /// Conservation ratio: fraction of the network's fragment locations that
    /// still hold known-live fragments.
    ///
    /// A value of `1.0` means all `n × num_chunks` fragments are in place.
    /// Below `k / n` the file cannot be recovered.
    pub fn conservation_ratio(&self) -> f64 {
        let total = self.n * self.num_chunks;
        if total == 0 {
            return 0.0;
        }
        let present: usize = self.chunks.iter().map(|c| c.fragment_locations.len()).sum();
        present as f64 / total as f64
    }

    /// Whether every chunk has at least `k` fragment locations recorded.
    pub fn is_recoverable(&self) -> bool {
        self.chunks
            .iter()
            .all(|c| c.fragment_locations.len() >= self.k)
    }

    /// Serialise to a compact JSON byte blob (for gossip / DHT storage).
    pub fn to_json_bytes(&self) -> BpResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(BpError::Serde)
    }

    /// Deserialise from a JSON byte blob.
    pub fn from_json_bytes(bytes: &[u8]) -> BpResult<Self> {
        serde_json::from_slice(bytes).map_err(BpError::Serde)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(network: &str) -> NetworkMetaKey {
        NetworkMetaKey::for_network(network)
    }

    // ── NetworkMetaKey ────────────────────────────────────────────────────

    #[test]
    fn key_deterministic_per_network() {
        assert_eq!(make_key("amici"), make_key("amici"));
        assert_ne!(make_key("amici"), make_key("lavoro"));
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = make_key("amici");
        let msg = b"my secret filename.txt";
        let blob = key.encrypt(msg);
        let back = key.decrypt(&blob).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn encrypt_different_nonces() {
        let key = make_key("amici");
        let msg = b"same plaintext";
        let a = key.encrypt(msg);
        let b = key.encrypt(msg);
        // Different nonces → different ciphertexts
        assert_ne!(a, b);
        // But both decrypt correctly
        assert_eq!(key.decrypt(&a).unwrap(), msg);
        assert_eq!(key.decrypt(&b).unwrap(), msg);
    }

    #[test]
    fn decrypt_wrong_key_fails() {
        let key1 = make_key("amici");
        let key2 = make_key("lavoro");
        let blob = key1.encrypt(b"hello");
        assert!(key2.decrypt(&blob).is_err());
    }

    #[test]
    fn decrypt_tampered_mac_fails() {
        let key = make_key("amici");
        let mut blob = key.encrypt(b"hello");
        // Flip a byte in the MAC region (bytes 16..48)
        blob[20] ^= 0xFF;
        assert!(key.decrypt(&blob).is_err());
    }

    #[test]
    fn decrypt_too_short_fails() {
        let key = make_key("amici");
        assert!(key.decrypt(&[0u8; 47]).is_err());
    }

    // ── FileManifest helpers ──────────────────────────────────────────────

    fn make_manifest(network_id: &str, filename: &str) -> FileManifest {
        let key = NetworkMetaKey::for_network(network_id);
        let mut m = FileManifest {
            file_id: uuid::Uuid::new_v4().to_string(),
            owner_fingerprint: "a3f19c2b".into(),
            network_id: network_id.into(),
            encrypted_name: vec![],
            size_bytes: 4_096_000,
            chunk_size: 1_048_576,
            num_chunks: 4,
            k: 3,
            n: 6,
            q: 1.0,
            ph: 0.999,
            pe: 0.999,
            chunks: vec![],
            created_at: 1_710_000_000,
        };
        m.set_name(filename, &key);
        m
    }

    #[test]
    fn set_and_decrypt_name() {
        let key = NetworkMetaKey::for_network("amici");
        let manifest = make_manifest("amici", "holiday_photos.tar");
        let name = manifest.decrypt_name(&key).unwrap();
        assert_eq!(name, "holiday_photos.tar");
    }

    #[test]
    fn conservation_ratio_all_fragments_present() {
        let mut m = make_manifest("amici", "file.bin");
        m.num_chunks = 2;
        m.n = 3;
        m.chunks = vec![
            ChunkManifest {
                chunk_id: "aaa".into(),
                chunk_index: 0,
                original_size: 1_048_576,
                fragment_locations: vec![
                    FragmentLocation {
                        fragment_id: "f1".into(),
                        pouch_peer_id: "p1".into(),
                        coding_vector: vec![1, 2, 3],
                    },
                    FragmentLocation {
                        fragment_id: "f2".into(),
                        pouch_peer_id: "p2".into(),
                        coding_vector: vec![4, 5, 6],
                    },
                    FragmentLocation {
                        fragment_id: "f3".into(),
                        pouch_peer_id: "p3".into(),
                        coding_vector: vec![7, 8, 9],
                    },
                ],
            },
            ChunkManifest {
                chunk_id: "bbb".into(),
                chunk_index: 1,
                original_size: 1_048_576,
                fragment_locations: vec![
                    FragmentLocation {
                        fragment_id: "f4".into(),
                        pouch_peer_id: "p1".into(),
                        coding_vector: vec![1, 0, 0],
                    },
                    FragmentLocation {
                        fragment_id: "f5".into(),
                        pouch_peer_id: "p2".into(),
                        coding_vector: vec![0, 1, 0],
                    },
                    FragmentLocation {
                        fragment_id: "f6".into(),
                        pouch_peer_id: "p3".into(),
                        coding_vector: vec![0, 0, 1],
                    },
                ],
            },
        ];
        assert!((m.conservation_ratio() - 1.0).abs() < 1e-9);
        assert!(m.is_recoverable());
    }

    #[test]
    fn conservation_ratio_partial() {
        let mut m = make_manifest("amici", "file.bin");
        m.num_chunks = 1;
        m.k = 3;
        m.n = 6;
        m.chunks = vec![ChunkManifest {
            chunk_id: "xxx".into(),
            chunk_index: 0,
            original_size: 512,
            fragment_locations: vec![
                FragmentLocation {
                    fragment_id: "a".into(),
                    pouch_peer_id: "p1".into(),
                    coding_vector: vec![],
                },
                FragmentLocation {
                    fragment_id: "b".into(),
                    pouch_peer_id: "p2".into(),
                    coding_vector: vec![],
                },
            ],
        }];
        let ratio = m.conservation_ratio();
        assert!((ratio - 2.0 / 6.0).abs() < 1e-9);
        // Only 2 fragments < k=3 → not recoverable
        assert!(!m.is_recoverable());
    }

    #[test]
    fn json_serialization_roundtrip() {
        let m = make_manifest("amici", "document.pdf");
        let bytes = m.to_json_bytes().unwrap();
        let back = FileManifest::from_json_bytes(&bytes).unwrap();
        assert_eq!(back.file_id, m.file_id);
        assert_eq!(back.encrypted_name, m.encrypted_name);
        assert_eq!(back.k, m.k);
        assert_eq!(back.ph, m.ph);
    }
}
