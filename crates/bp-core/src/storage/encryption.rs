//! Per-chunk encryption for BillPouch storage ("encryption at rest").
//!
//! ## Overview
//!
//! Before a chunk is RLNC-encoded and distributed across Pouch peers, it is
//! encrypted with a **network-scoped symmetric key** using ChaCha20-Poly1305.
//! This ensures that Pouch nodes holding fragments never have access to
//! plaintext data — they only ever see and forward opaque ciphertext.
//!
//! ## Key derivation
//!
//! The per-chunk encryption key is derived from the network metadata key:
//!
//! ```text
//! chunk_key = BLAKE3_keyed(network_meta_key, "billpouch/chunk-enc/v1")
//! ```
//!
//! All nodes in the same network can re-derive this key autonomously without
//! any additional key exchange.  The key is **network-scoped** — two chunks
//! in the same network share the same key, but different nonces provide
//! per-ciphertext IND-CPA security.
//!
//! ## On-disk / on-wire format
//!
//! ```text
//! [ nonce (12 bytes) | ciphertext + poly1305 tag (len + 16 bytes) ]
//! ```
//!
//! The nonce is randomly generated at encryption time.  The total overhead is
//! 28 bytes per chunk (12-byte nonce + 16-byte authentication tag).

use crate::{
    error::{BpError, BpResult},
    storage::manifest::NetworkMetaKey,
};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::RngCore;

/// Nonce length for ChaCha20-Poly1305.
const NONCE_LEN: usize = 12;

/// `ChunkCipher` wraps the network-scoped ChaCha20-Poly1305 key that is used
/// to encrypt and decrypt chunk data before/after RLNC coding.
///
/// Create one instance per upload/download operation via
/// [`ChunkCipher::for_network`].
pub struct ChunkCipher {
    cipher: ChaCha20Poly1305,
}

impl ChunkCipher {
    /// Derive the chunk encryption key for `network_id` and build a cipher.
    ///
    /// ```text
    /// chunk_key = BLAKE3_keyed(NetworkMetaKey(network_id), "billpouch/chunk-enc/v1")
    /// ```
    pub fn for_network(network_id: &str) -> Self {
        let net_key = NetworkMetaKey::for_network(network_id);
        Self::from_meta_key(&net_key)
    }

    /// Build a cipher from an already-computed [`NetworkMetaKey`].
    pub fn from_meta_key(meta_key: &NetworkMetaKey) -> Self {
        // Derive a distinct 32-byte key for chunk encryption.
        let mut h = blake3::Hasher::new_keyed(&meta_key.0);
        h.update(b"billpouch/chunk-enc/v1");
        let chunk_key: [u8; 32] = *h.finalize().as_bytes();
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&chunk_key));
        Self { cipher }
    }

    /// Encrypt `plaintext` and return `nonce(12) || ciphertext_with_tag(len+16)`.
    ///
    /// A fresh random nonce is generated for every call, guaranteeing that
    /// repeated encryptions of the same plaintext produce different outputs.
    pub fn encrypt(&self, plaintext: &[u8]) -> BpResult<Vec<u8>> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| BpError::Storage(format!("Chunk encryption failed: {e}")))?;

        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Decrypt a blob produced by [`encrypt`](Self::encrypt).
    ///
    /// Returns the original plaintext or an error if the tag verification
    /// fails (wrong key, wrong network, or corrupted data).
    pub fn decrypt(&self, blob: &[u8]) -> BpResult<Vec<u8>> {
        if blob.len() < NONCE_LEN + 16 {
            return Err(BpError::Storage(
                "Encrypted chunk blob is too short (corrupted?)".into(),
            ));
        }
        let nonce = Nonce::from_slice(&blob[..NONCE_LEN]);
        let ciphertext = &blob[NONCE_LEN..];
        self.cipher.decrypt(nonce, ciphertext).map_err(|_| {
            BpError::Storage(
                "Chunk decryption failed — wrong network key or corrupted data".into(),
            )
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher(net: &str) -> ChunkCipher {
        ChunkCipher::for_network(net)
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let c = cipher("amici");
        let data = b"Hello, BillPouch! This is a test chunk payload.";
        let blob = c.encrypt(data).unwrap();
        let back = c.decrypt(&blob).unwrap();
        assert_eq!(back.as_slice(), data.as_ref());
    }

    #[test]
    fn different_nonces_per_call() {
        let c = cipher("amici");
        let data = b"same plaintext";
        let blob1 = c.encrypt(data).unwrap();
        let blob2 = c.encrypt(data).unwrap();
        // Different nonces → different ciphertexts
        assert_ne!(blob1, blob2);
        // Both still decrypt correctly
        assert_eq!(c.decrypt(&blob1).unwrap(), data);
        assert_eq!(c.decrypt(&blob2).unwrap(), data);
    }

    #[test]
    fn wrong_network_decryption_fails() {
        let c1 = cipher("amici");
        let c2 = cipher("lavoro");
        let blob = c1.encrypt(b"secret data").unwrap();
        assert!(
            c2.decrypt(&blob).is_err(),
            "Decryption with wrong network key must fail"
        );
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let c = cipher("amici");
        let mut blob = c.encrypt(b"original data").unwrap();
        // Flip a byte inside the ciphertext (after the 12-byte nonce).
        blob[NONCE_LEN + 3] ^= 0xAA;
        assert!(c.decrypt(&blob).is_err(), "Tampered ciphertext must fail auth");
    }

    #[test]
    fn too_short_blob_fails() {
        let c = cipher("amici");
        assert!(c.decrypt(&[0u8; 20]).is_err());
        assert!(c.decrypt(&[]).is_err());
    }

    #[test]
    fn network_keys_are_distinct() {
        // Two networks must produce different keys so data is isolated.
        let c1 = cipher("amici");
        let c2 = cipher("lavoro");
        let plain = b"cross-network probe";
        let blob1 = c1.encrypt(plain).unwrap();
        // Can't decrypt blob1 with c2 (different keys)
        assert!(c2.decrypt(&blob1).is_err());
        // Can't decrypt blob1 with wrong nonce padding (blob is not valid for c2)
        let blob2 = c2.encrypt(plain).unwrap();
        assert!(c1.decrypt(&blob2).is_err());
    }
}
