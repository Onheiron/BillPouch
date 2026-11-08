use crate::error::{BpError, BpResult};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::RngCore;

const NONCE_LEN: usize = 12;

pub struct ChunkCipher {
    cipher: ChaCha20Poly1305,
}

impl ChunkCipher {
    pub fn from_raw_key(key: [u8; 32]) -> Self {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        Self { cipher }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_ne!(blob1, blob2);
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
}
