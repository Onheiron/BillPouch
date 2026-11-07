use crate::{
    config,
    error::{BpError, BpResult},
    identity::{fingerprint_pubkey, Identity},
    storage::network_key::NetworkMetaKey,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitePayload {
    pub version: u8,
    pub network_id: String,
    pub network_meta_key_hex: String,
    pub inviter_fingerprint: String,
    pub inviter_pubkey_hex: String,
    pub invitee_fingerprint: Option<String>,
    pub expires_at: u64,
    pub nonce_hex: String,
}

pub fn create_invite(
    identity: &Identity,
    network_id: &str,
    invitee_fingerprint: Option<String>,
    ttl_hours: u64,
    invite_password: &str,
) -> BpResult<String> {
    let nmk = NetworkMetaKey::load(network_id)?.ok_or_else(|| {
        BpError::Config(format!(
            "No network key for '{network_id}' — join this network first"
        ))
    })?;

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

    let signature = identity
        .keypair
        .sign(&payload_json)
        .map_err(|e| BpError::Identity(format!("Signing failed: {e}")))?;

    let mut plaintext: Vec<u8> = Vec::with_capacity(4 + payload_json.len() + signature.len());
    plaintext.extend_from_slice(&(payload_json.len() as u32).to_le_bytes());
    plaintext.extend_from_slice(&payload_json);
    plaintext.extend_from_slice(&signature);

    let mut salt = [0u8; ARGON2_SALT_LEN];
    let mut enc_nonce = [0u8; CHACHA_NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut enc_nonce);

    let key = derive_key(invite_password, &salt)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&enc_nonce), plaintext.as_ref())
        .map_err(|e| BpError::Identity(format!("Invite encryption failed: {e}")))?;

    let mut blob_bytes = Vec::with_capacity(ARGON2_SALT_LEN + CHACHA_NONCE_LEN + ciphertext.len());
    blob_bytes.extend_from_slice(&salt);
    blob_bytes.extend_from_slice(&enc_nonce);
    blob_bytes.extend_from_slice(&ciphertext);

    Ok(hex::encode(blob_bytes))
}

pub fn redeem_invite(blob: &str, invite_password: &str) -> BpResult<InvitePayload> {
    let blob_bytes = hex::decode(blob)
        .map_err(|e| BpError::Config(format!("Invalid invite blob (not hex): {e}")))?;

    let min_len = ARGON2_SALT_LEN + CHACHA_NONCE_LEN + 16;
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

    if plaintext.len() < 4 {
        return Err(BpError::Config("Invite plaintext too short".into()));
    }
    let payload_len = u32::from_le_bytes(plaintext[..4].try_into().unwrap()) as usize;
    if plaintext.len() < 4 + payload_len {
        return Err(BpError::Config("Invite payload truncated".into()));
    }
    let payload_json = &plaintext[4..4 + payload_len];
    let signature = &plaintext[4 + payload_len..];

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

    let expected_fp = fingerprint_pubkey(&pubkey);
    if expected_fp != payload.inviter_fingerprint {
        return Err(BpError::Identity(
            "Invite fingerprint mismatch — token may be tampered".into(),
        ));
    }

    let now = chrono::Utc::now().timestamp() as u64;
    if now > payload.expires_at {
        return Err(BpError::Identity(format!(
            "Invite token expired at unix timestamp {}",
            payload.expires_at
        )));
    }

    Ok(payload)
}

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

fn derive_key(password: &str, salt: &[u8]) -> BpResult<[u8; 32]> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| BpError::Identity(format!("KDF error: {e}")))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Identity, UserProfile};
    use crate::storage::network_key::NetworkMetaKey;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn make_identity_and_key() -> (Identity, NetworkMetaKey) {
        let nmk = NetworkMetaKey([0xABu8; 32]);
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

    fn with_temp_nmk<F: FnOnce(&Identity, &str)>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir =
            std::env::temp_dir().join(format!("bp_invite_test_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let orig_home = std::env::var("HOME").ok();

        std::env::set_var("HOME", &dir);
        std::env::set_var("XDG_DATA_HOME", dir.join(".local/share").to_str().unwrap());

        let network_id = "test-net";
        let (identity, nmk) = make_identity_and_key();
        nmk.save(network_id).unwrap();

        f(&identity, network_id);

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
            let blob = create_invite(identity, network_id, None, 0, "pw").unwrap();
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

            save_invite_key(&payload).unwrap();

            let loaded = NetworkMetaKey::load(network_id).unwrap().unwrap();
            assert_eq!(loaded.0, [0xABu8; 32]);
        });
    }
}
