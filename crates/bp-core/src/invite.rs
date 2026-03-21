//! Signed + password-encrypted invite tokens for network access control.
//!
//! ## Design
//!
//! A BillPouch network is private by default.  The only way to join is to
//! receive an invite token from an existing member.  The token:
//!
//! 1. **Contains the `NetworkMetaKey`** — the 32-byte random secret that
//!    protects file metadata on the network.
//! 2. **Is signed** by the inviter's Ed25519 key so the recipient can verify
//!    the invite is authentic.
//! 3. **Is encrypted** with a password shared out-of-band (Signal, phone call,
//!    etc.).  The password is never stored anywhere.
//!
//! ## Wire format
//!
//! ```text
//! hex(
//!   salt(16)            — Argon2id salt for password KDF
//!   || nonce(12)        — ChaCha20-Poly1305 nonce
//!   || ciphertext       — encrypt(
//!          payload_len(4, LE u32)
//!          || payload_json
//!          || ed25519_signature(64)
//!      )
//! )
//! ```
//!
//! ## Usage
//!
//! Inviter:
//! ```no_run
//! # use bp_core::invite::create_invite;
//! # use bp_core::identity::Identity;
//! # let identity = Identity::load(None).unwrap();
//! let blob = create_invite(&identity, "amici", None, 24, "shared-password").unwrap();
//! println!("{blob}");
//! ```
//!
//! Invitee:
//! ```no_run
//! # use bp_core::invite::{redeem_invite, save_invite_key};
//! let payload = redeem_invite("<blob>", "shared-password").unwrap();
//! save_invite_key(&payload).unwrap();  // writes NetworkMetaKey to disk
//! ```

use crate::{
    config,
    error::{BpError, BpResult},
    identity::{fingerprint_pubkey, Identity},
    storage::manifest::NetworkMetaKey,
};
use argon2::Argon2;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};

const ARGON2_SALT_LEN: usize = 16;
const CHACHA_NONCE_LEN: usize = 12;

// ── Payload ───────────────────────────────────────────────────────────────────

/// The plaintext content of an invite token.
///
/// Serialised to JSON before signing and encryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitePayload {
    /// Format version — currently `1`.
    pub version: u8,
    /// Network the invitee is being invited to.
    pub network_id: String,
    /// Hex-encoded 32-byte `NetworkMetaKey` for `network_id`.
    ///
    /// **This is the secret** — it must never be transmitted in plaintext.
    pub network_meta_key_hex: String,
    /// Hex fingerprint of the inviter (for display / audit).
    pub inviter_fingerprint: String,
    /// Hex-encoded protobuf-serialised public key of the inviter.
    ///
    /// Included so the recipient can verify the signature without already
    /// being in the network.
    pub inviter_pubkey_hex: String,
    /// Optional: if `Some`, only this fingerprint should redeem the token.
    /// `None` = open invite (anyone who has the password can redeem it).
    pub invitee_fingerprint: Option<String>,
    /// Unix timestamp after which the token is considered expired.
    pub expires_at: u64,
    /// Random 16-byte hex nonce — prevents replay of the same token.
    pub nonce_hex: String,
}

// ── Create ────────────────────────────────────────────────────────────────────

/// Generate a signed + password-encrypted invite token for `network_id`.
///
/// # Parameters
/// - `identity`             — The inviter's loaded identity (must have joined `network_id`).
/// - `network_id`           — Network the invitee will be joining.
/// - `invitee_fingerprint`  — Optional: restrict the token to one specific invitee.
/// - `ttl_hours`            — How long the token is valid (e.g. `24`).
/// - `invite_password`      — Shared out-of-band with the invitee (never stored).
///
/// # Returns
/// A hex-encoded blob that can be passed to [`redeem_invite`].
pub fn create_invite(
    identity: &Identity,
    network_id: &str,
    invitee_fingerprint: Option<String>,
    ttl_hours: u64,
    invite_password: &str,
) -> BpResult<String> {
    // Load the NetworkMetaKey — must exist (node has previously joined/created the network).
    let nmk = NetworkMetaKey::load(network_id)?.ok_or_else(|| {
        BpError::Config(format!(
            "No network key for '{network_id}' — join this network first"
        ))
    })?;

    // Build random nonce (prevents token replay).
    let mut nonce_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let expires_at =
        (chrono::Utc::now() + chrono::Duration::hours(ttl_hours as i64)).timestamp() as u64;

    let payload = InvitePayload {
        version: 1,
        network_id: network_id.to_string(),
        network_meta_key_hex: hex::encode(nmk.0),
        inviter_fingerprint: identity.fingerprint.clone(),
        inviter_pubkey_hex: hex::encode(identity.keypair.public().encode_protobuf()),
        invitee_fingerprint,
        expires_at,
        nonce_hex: hex::encode(nonce_bytes),
    };

    let payload_json = serde_json::to_vec(&payload).map_err(BpError::Serde)?;

    // Ed25519 sign the payload.
    let signature = identity
        .keypair
        .sign(&payload_json)
        .map_err(|e| BpError::Identity(format!("Signing failed: {e}")))?;

    // Build plaintext: len(4 LE) || payload_json || signature.
    let mut plaintext: Vec<u8> = Vec::with_capacity(4 + payload_json.len() + signature.len());
    plaintext.extend_from_slice(&(payload_json.len() as u32).to_le_bytes());
    plaintext.extend_from_slice(&payload_json);
    plaintext.extend_from_slice(&signature);

    // Encrypt with Argon2id-derived key.
    let mut salt = [0u8; ARGON2_SALT_LEN];
    let mut enc_nonce = [0u8; CHACHA_NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut enc_nonce);

    let key = derive_key(invite_password, &salt)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&enc_nonce), plaintext.as_ref())
        .map_err(|e| BpError::Identity(format!("Invite encryption failed: {e}")))?;

    // Assemble blob: salt || enc_nonce || ciphertext.
    let mut blob_bytes = Vec::with_capacity(ARGON2_SALT_LEN + CHACHA_NONCE_LEN + ciphertext.len());
    blob_bytes.extend_from_slice(&salt);
    blob_bytes.extend_from_slice(&enc_nonce);
    blob_bytes.extend_from_slice(&ciphertext);

    Ok(hex::encode(blob_bytes))
}

// ── Redeem ────────────────────────────────────────────────────────────────────

/// Decrypt, verify and parse an invite token.
///
/// Does **not** save anything to disk — call [`save_invite_key`] afterwards.
///
/// # Errors
/// Returns an error if:
/// - The blob is malformed or truncated.
/// - The password is wrong (ChaCha20-Poly1305 authentication fails).
/// - The Ed25519 signature is invalid.
/// - The token has expired.
pub fn redeem_invite(blob: &str, invite_password: &str) -> BpResult<InvitePayload> {
    let blob_bytes = hex::decode(blob)
        .map_err(|e| BpError::Config(format!("Invalid invite blob (not hex): {e}")))?;

    let min_len = ARGON2_SALT_LEN + CHACHA_NONCE_LEN + 16; // 16 = ChaCha20Poly1305 tag
    if blob_bytes.len() < min_len {
        return Err(BpError::Config("Invite blob is too short".into()));
    }

    let salt: [u8; ARGON2_SALT_LEN] = blob_bytes[..ARGON2_SALT_LEN].try_into().unwrap();
    let enc_nonce: [u8; CHACHA_NONCE_LEN] = blob_bytes
        [ARGON2_SALT_LEN..ARGON2_SALT_LEN + CHACHA_NONCE_LEN]
        .try_into()
        .unwrap();
    let ciphertext = &blob_bytes[ARGON2_SALT_LEN + CHACHA_NONCE_LEN..];

    let key = derive_key(invite_password, &salt)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&enc_nonce), ciphertext)
        .map_err(|_| BpError::Identity("Wrong invite password or corrupted token".into()))?;

    // Parse: len(4) || payload_json || signature.
    if plaintext.len() < 4 {
        return Err(BpError::Config("Invite plaintext too short".into()));
    }
    let payload_len = u32::from_le_bytes(plaintext[..4].try_into().unwrap()) as usize;
    if plaintext.len() < 4 + payload_len {
        return Err(BpError::Config("Invite payload truncated".into()));
    }
    let payload_json = &plaintext[4..4 + payload_len];
    let signature = &plaintext[4 + payload_len..];

    // Verify Ed25519 signature.
    let payload: InvitePayload = serde_json::from_slice(payload_json).map_err(BpError::Serde)?;

    let pubkey_bytes = hex::decode(&payload.inviter_pubkey_hex)
        .map_err(|e| BpError::Identity(format!("Invalid inviter pubkey hex: {e}")))?;
    let pubkey = libp2p::identity::PublicKey::try_decode_protobuf(&pubkey_bytes)
        .map_err(|e| BpError::Identity(format!("Cannot parse inviter pubkey: {e}")))?;

    if !pubkey.verify(payload_json, signature) {
        return Err(BpError::Identity(
            "Invite signature verification failed — token may be forged".into(),
        ));
    }

    // Verify fingerprint matches the public key.
    let expected_fp = fingerprint_pubkey(&pubkey);
    if expected_fp != payload.inviter_fingerprint {
        return Err(BpError::Identity(
            "Invite fingerprint mismatch — token may be tampered".into(),
        ));
    }

    // Check expiry.
    let now = chrono::Utc::now().timestamp() as u64;
    if now > payload.expires_at {
        return Err(BpError::Identity(format!(
            "Invite token expired at unix timestamp {}",
            payload.expires_at
        )));
    }

    Ok(payload)
}

/// Persist the `NetworkMetaKey` from a redeemed invite to local storage.
///
/// Must be called after [`redeem_invite`] succeeds.  After this call,
/// `NetworkMetaKey::load(&payload.network_id)` will return `Some(key)`.
pub fn save_invite_key(payload: &InvitePayload) -> BpResult<()> {
    config::ensure_dirs()?;
    let key_bytes = hex::decode(&payload.network_meta_key_hex)
        .map_err(|e| BpError::Config(format!("Invalid network key in invite: {e}")))?;
    if key_bytes.len() != 32 {
        return Err(BpError::Config(
            "Network key in invite has wrong length".into(),
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&key_bytes);
    let nmk = NetworkMetaKey(arr);
    nmk.save(&payload.network_id)?;
    tracing::info!(
        network = %payload.network_id,
        inviter = %payload.inviter_fingerprint,
        "NetworkMetaKey saved from invite"
    );
    Ok(())
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn derive_key(password: &str, salt: &[u8]) -> BpResult<[u8; 32]> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| BpError::Identity(format!("KDF error: {e}")))?;
    Ok(key)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Identity, UserProfile};
    use crate::storage::manifest::NetworkMetaKey;

    fn make_identity_and_key() -> (Identity, NetworkMetaKey) {
        // Use a deterministic test key (not from disk).
        let nmk = NetworkMetaKey([0xABu8; 32]);
        // Build a fresh keypair in memory.
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        let peer_id = libp2p::PeerId::from_public_key(&keypair.public());
        let fp = crate::identity::fingerprint(&keypair);
        let profile = UserProfile {
            fingerprint: fp.clone(),
            alias: Some("tester".into()),
            created_at: chrono::Utc::now(),
        };
        let identity = Identity {
            keypair,
            peer_id,
            fingerprint: fp,
            profile,
        };
        (identity, nmk)
    }

    /// Write a fake NetworkMetaKey for `network_id` to a temp dir and update
    /// the config base path so `load/save` use it.
    fn with_temp_nmk<F: FnOnce(&Identity, &str)>(f: F) {
        let dir =
            std::env::temp_dir().join(format!("bp_invite_test_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let orig_home = std::env::var("HOME").ok();

        // Point HOME to the temp dir so config::base_dir() uses it.
        std::env::set_var("HOME", &dir);
        std::env::set_var("XDG_DATA_HOME", dir.join(".local/share").to_str().unwrap());

        let network_id = "test-net";
        let (identity, nmk) = make_identity_and_key();
        nmk.save(network_id).unwrap();

        f(&identity, network_id);

        // Restore
        match orig_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        std::env::remove_var("XDG_DATA_HOME");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn create_and_redeem_roundtrip() {
        with_temp_nmk(|identity, network_id| {
            let blob = create_invite(identity, network_id, None, 24, "test-password").unwrap();
            let payload = redeem_invite(&blob, "test-password").unwrap();
            assert_eq!(payload.network_id, network_id);
            assert_eq!(payload.inviter_fingerprint, identity.fingerprint);
            assert_eq!(payload.network_meta_key_hex, hex::encode([0xABu8; 32]));
        });
    }

    #[test]
    #[cfg(unix)]
    fn wrong_password_fails() {
        with_temp_nmk(|identity, network_id| {
            let blob = create_invite(identity, network_id, None, 24, "correct").unwrap();
            assert!(redeem_invite(&blob, "wrong").is_err());
        });
    }

    #[test]
    #[cfg(unix)]
    fn expired_token_fails() {
        with_temp_nmk(|identity, network_id| {
            // TTL = 0 hours → already expired
            let blob = create_invite(identity, network_id, None, 0, "pw").unwrap();
            // Give clock 1s buffer — token created with expires_at = now, which may be ≤ now
            std::thread::sleep(std::time::Duration::from_secs(1));
            assert!(redeem_invite(&blob, "pw").is_err());
        });
    }

    #[test]
    #[cfg(unix)]
    fn tampered_blob_fails() {
        with_temp_nmk(|identity, network_id| {
            let mut blob_bytes =
                hex::decode(create_invite(identity, network_id, None, 24, "pw").unwrap()).unwrap();
            // Flip a byte in the ciphertext region.
            let last = blob_bytes.len() - 5;
            blob_bytes[last] ^= 0xFF;
            assert!(redeem_invite(&hex::encode(blob_bytes), "pw").is_err());
        });
    }

    #[test]
    #[cfg(unix)]
    fn save_invite_key_persists() {
        with_temp_nmk(|identity, network_id| {
            let blob = create_invite(identity, network_id, None, 24, "pw").unwrap();
            let payload = redeem_invite(&blob, "pw").unwrap();

            // Save should not fail.
            save_invite_key(&payload).unwrap();

            // The key should now be loadable.
            let loaded = NetworkMetaKey::load(network_id).unwrap().unwrap();
            assert_eq!(loaded.0, [0xABu8; 32]);
        });
    }
}
