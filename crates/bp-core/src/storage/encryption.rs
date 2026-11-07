//! Per-chunk encryption for BillPouch storage ("encryption at rest").
//!
//! ## Overview
//!
//! Before a chunk is RLNC-encoded and distributed across Pouch peers, it is
//! encrypted with a **per-user Content Encryption Key (CEK)** using
//! ChaCha20-Poly1305.  This ensures that:
//!
//! 1. Pouch nodes holding fragments never have access to plaintext data.
//! 2. Even nodes in the same network cannot read files owned by other users.
//!
//! ## CEK derivation
//!
//! ```text
//! cek = BLAKE3_keyed(identity.secret_material(),
//!                    "billpouch/cek/v1" || BLAKE3(plaintext_chunk))
//! ```
//!
//! The CEK is deterministic for a given `(identity, plaintext chunk)` pair.
//! The daemon persists `chunk_id → plaintext_hash` in `cek_hints.json` so that
//! [`ChunkCipher::for_user`] can be re-derived at GetFile time.
//!
//! ## On-disk / on-wire format
//!
//! ```text
//! [ nonce (12 bytes) | ciphertext + poly1305 tag (len + 16 bytes) ]
//! ```
//!
//! The nonce is randomly generated at encryption time.  The total overhead is
//! 28 bytes per chunk (12-byte nonce + 16-byte authentication tag).

use crate::error::{BpError, BpResult};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::RngCore;

/// Nonce length for ChaCha20-Poly1305.
const NONCE_LEN: usize = 12;

/// `ChunkCipher` wraps the ChaCha20-Poly1305 key used to encrypt and decrypt
/// chunk data before/after RLNC coding.
///
/// **Production path:** create via [`ChunkCipher::for_user`] which derives a
/// CEK from the owner’s identity secret and the plaintext chunk hash.
pub struct ChunkCipher {
    cipher: ChaCha20Poly1305,
}

impl ChunkCipher {
    /// Derive the Content Encryption Key for a chunk and build a cipher.
    ///
    /// `secret_material` comes from [`crate::identity::Identity::secret_material`].
    /// `plaintext_hash` is `BLAKE3(chunk_data)` computed before encryption.
    ///
    /// ```text
    /// cek = BLAKE3_keyed(secret_material, "billpouch/cek/v1" || plaintext_hash)
    /// ```
    pub fn for_user(secret_material: &[u8; 32], plaintext_hash: &[u8; 32]) -> Self {
        let mut h = blake3::Hasher::new_keyed(secret_material);
        h.update(b"billpouch/cek/v1");
        h.update(plaintext_hash);
        let cek: [u8; 32] = *h.finalize().as_bytes();
        Self::from_raw_key(cek)
    }

    /// Build a cipher from a raw 32-byte key.
    pub fn from_raw_key(key: [u8; 32]) -> Self {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
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
    /// fails (wrong key or corrupted data).
    pub fn decrypt(&self, blob: &[u8]) -> BpResult<Vec<u8>> {
        if blob.len() < NONCE_LEN + 16 {
            return Err(BpError::Storage(
                "Encrypted chunk blob is too short (corrupted?)".into(),
            ));
        }
        let nonce = Nonce::from_slice(&blob[..NONCE_LEN]);
        let ciphertext = &blob[NONCE_LEN..];
        self.cipher.decrypt(nonce, ciphertext).map_err(|_| {
            BpError::Storage("Chunk decryption failed — wrong key or corrupted data".into())
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed-key cipher for tests — isolates cipher mechanics from key management.
    fn cipher(label: &str) -> ChunkCipher {
        let key = *blake3::Hasher::new()
            .update(b"bp-test-cipher")
            .update(label.as_bytes())
            .finalize()
            .as_bytes();
        ChunkCipher::from_raw_key(key)
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
    fn wrong_key_decryption_fails() {
        let c1 = cipher("amici");
        let c2 = cipher("lavoro");
        let blob = c1.encrypt(b"secret data").unwrap();
        assert!(
            c2.decrypt(&blob).is_err(),
            "Decryption with wrong key must fail"
        );
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let c = cipher("amici");
        let mut blob = c.encrypt(b"original data").unwrap();
        // Flip a byte inside the ciphertext (after the 12-byte nonce).
        blob[NONCE_LEN + 3] ^= 0xAA;
        assert!(
            c.decrypt(&blob).is_err(),
            "Tampered ciphertext must fail auth"
        );
    }

    #[test]
    fn too_short_blob_fails() {
        let c = cipher("amici");
        assert!(c.decrypt(&[0u8; 20]).is_err());
        assert!(c.decrypt(&[]).is_err());
    }

    #[test]
    fn for_user_produces_distinct_ceks() {
        // Two users with different secret material encrypt the same plaintext.
        let sm1 = [0x11u8; 32];
        let sm2 = [0x22u8; 32];
        let ph = *blake3::hash(b"my chunk").as_bytes();
        let c1 = ChunkCipher::for_user(&sm1, &ph);
        let c2 = ChunkCipher::for_user(&sm2, &ph);
        let blob = c1.encrypt(b"secret").unwrap();
        // User 2 cannot decrypt user 1’s data.
        assert!(c2.decrypt(&blob).is_err());
    }

    #[test]
    fn for_user_same_input_same_cek() {
        let sm = [0xAAu8; 32];
        let ph = *blake3::hash(b"chunk content").as_bytes();
        let c1 = ChunkCipher::for_user(&sm, &ph);
        let c2 = ChunkCipher::for_user(&sm, &ph);
        let blob = c1.encrypt(b"data").unwrap();
        // Re-derived cipher decrypts correctly.
        assert_eq!(c2.decrypt(&blob).unwrap(), b"data");
    }
}
