//! User identity: Ed25519 keypair persisted to disk.
//!
//! The keypair can be stored in two formats:
//! - **Plaintext** (`identity.key`) — raw protobuf bytes, no passphrase.
//! - **Encrypted** (`identity.key.enc`) — JSON envelope produced by Argon2id
//!   key-derivation + ChaCha20-Poly1305 symmetric encryption.  Used when the
//!   user supplies `--passphrase` at `bp login`.
//!
//! The encrypted format is automatically detected at load time.  The
//! plaintext format remains supported for backwards compatibility and for
//! headless/CI environments.

use crate::{
    config,
    error::{BpError, BpResult},
};
use chacha20poly1305::{aead::{Aead, KeyInit}, ChaCha20Poly1305, Key, Nonce};
use argon2::Argon2;
use libp2p::identity::Keypair;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── Encryption constants ──────────────────────────────────────────────────────

const ARGON2_SALT_LEN: usize = 16; // 128-bit salt for Argon2id
const CHACHA_NONCE_LEN: usize = 12; // 96-bit nonce for ChaCha20-Poly1305
const CHACHA_KEY_LEN: usize = 32; // 256-bit key for ChaCha20-Poly1305

/// On-disk representation of a passphrase-protected keypair.
///
/// Written as pretty-printed JSON to `~/.local/share/billpouch/identity.key.enc`.
#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedKeyFile {
    /// Format version — currently `1`.
    pub version: u8,
    /// Hex-encoded 16-byte Argon2id salt.
    pub salt: String,
    /// Hex-encoded 12-byte ChaCha20-Poly1305 nonce.
    pub nonce: String,
    /// Hex-encoded ChaCha20-Poly1305 ciphertext (includes the 16-byte auth tag).
    pub ciphertext: String,
}

/// Derive a 32-byte ChaCha20 key from `passphrase` using Argon2id.
fn derive_key(passphrase: &str, salt: &[u8]) -> BpResult<[u8; CHACHA_KEY_LEN]> {
    let mut key = [0u8; CHACHA_KEY_LEN];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| BpError::Identity(format!("KDF error: {e}")))?;
    Ok(key)
}

/// Encrypt raw protobuf-encoded keypair bytes with `passphrase`.
fn encrypt_keypair_bytes(raw: &[u8], passphrase: &str) -> BpResult<EncryptedKeyFile> {
    let mut salt = [0u8; ARGON2_SALT_LEN];
    let mut nonce_bytes = [0u8; CHACHA_NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key = derive_key(passphrase, &salt)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, raw)
        .map_err(|e| BpError::Identity(format!("Encryption error: {e}")))?;

    Ok(EncryptedKeyFile {
        version: 1,
        salt: hex::encode(salt),
        nonce: hex::encode(nonce_bytes),
        ciphertext: hex::encode(ciphertext),
    })
}

/// Decrypt raw protobuf-encoded keypair bytes from an [`EncryptedKeyFile`].
///
/// Returns [`BpError::Identity`] if the passphrase is wrong or the file is
/// corrupted.
fn decrypt_keypair_bytes(enc: &EncryptedKeyFile, passphrase: &str) -> BpResult<Vec<u8>> {
    let salt =
        hex::decode(&enc.salt).map_err(|e| BpError::Identity(format!("Invalid salt: {e}")))?;
    let nonce_bytes =
        hex::decode(&enc.nonce).map_err(|e| BpError::Identity(format!("Invalid nonce: {e}")))?;
    let ciphertext = hex::decode(&enc.ciphertext)
        .map_err(|e| BpError::Identity(format!("Invalid ciphertext: {e}")))?;

    let key = derive_key(passphrase, &salt)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let nonce = Nonce::from_slice(&nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| BpError::Identity("Wrong passphrase or corrupted identity file".into()))
}

/// Human-readable fingerprint of a public key (hex-encoded SHA-256, first 16 bytes).
pub fn fingerprint(keypair: &Keypair) -> String {
    let pub_bytes = keypair.public().encode_protobuf();
    let hash = Sha256::digest(&pub_bytes);
    hex::encode(&hash[..8])
}

/// Persistent user metadata stored alongside the keypair.
///
/// Serialised as JSON to `~/.local/share/billpouch/profile.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    /// Hex fingerprint derived from this identity's public key (16 hex chars).
    pub fingerprint: String,
    /// Optional human-readable alias chosen at `bp login`.
    pub alias: Option<String>,
    /// UTC timestamp when the identity was first generated.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// In-memory identity: the loaded keypair and all derived or persisted metadata.
///
/// Constructed either by [`Identity::generate`] (first login) or
/// [`Identity::load`] (subsequent daemon starts).
///
/// `Identity` does **not** implement `Debug` because `libp2p::identity::Keypair`
/// does not expose a `Debug` impl (the bytes are considered secret).
#[derive(Clone)]
pub struct Identity {
    /// The Ed25519 keypair.  Treat as secret — never log or serialise directly.
    pub keypair: Keypair,
    /// libp2p peer identifier derived deterministically from the public key.
    pub peer_id: libp2p::PeerId,
    /// Hex-encoded SHA-256(pubkey)[0..8] — 16 characters, immutable for this identity.
    pub fingerprint: String,
    /// Persisted profile (alias, creation timestamp, etc.).
    pub profile: UserProfile,
}

impl Identity {
    /// Generate a brand-new Ed25519 identity and persist it.
    ///
    /// If `passphrase` is `Some`, the keypair is encrypted with Argon2id +
    /// ChaCha20-Poly1305 and written to `identity.key.enc`.  If `None`, the
    /// raw protobuf bytes are written to `identity.key` (backwards-compatible).
    pub fn generate(alias: Option<String>, passphrase: Option<&str>) -> BpResult<Self> {
        config::ensure_dirs()?;
        let keypair = Keypair::generate_ed25519();
        let peer_id = libp2p::PeerId::from_public_key(&keypair.public());
        let fp = fingerprint(&keypair);

        let profile = UserProfile {
            fingerprint: fp.clone(),
            alias,
            created_at: chrono::Utc::now(),
        };

        // Persist keypair — encrypted or plaintext.
        let key_bytes = keypair
            .to_protobuf_encoding()
            .map_err(|e| BpError::Identity(e.to_string()))?;
        if let Some(pass) = passphrase {
            let enc = encrypt_keypair_bytes(&key_bytes, pass)?;
            let json = serde_json::to_string_pretty(&enc)?;
            std::fs::write(config::encrypted_identity_path()?, json).map_err(BpError::Io)?;
            tracing::info!("New identity created (passphrase-protected) — fingerprint: {fp}");
        } else {
            std::fs::write(config::identity_path()?, &key_bytes).map_err(BpError::Io)?;
            tracing::info!("New identity created — fingerprint: {fp}");
        }

        // Persist profile.
        let json = serde_json::to_string_pretty(&profile)?;
        std::fs::write(config::profile_path()?, json).map_err(BpError::Io)?;

        Ok(Self {
            keypair,
            peer_id,
            fingerprint: fp,
            profile,
        })
    }

    /// Load existing identity from disk.
    ///
    /// - If `identity.key.enc` exists, `passphrase` is **required**.  Passing
    ///   `None` returns [`BpError::Identity`] with a descriptive message.
    /// - If `identity.key` exists (plaintext), the `passphrase` argument is
    ///   ignored.
    /// - If neither file exists, returns [`BpError::NotAuthenticated`].
    pub fn load(passphrase: Option<&str>) -> BpResult<Self> {
        let enc_path = config::encrypted_identity_path()?;
        let plain_path = config::identity_path()?;

        let key_bytes = if enc_path.exists() {
            // Encrypted identity — passphrase is mandatory.
            let pass = passphrase.ok_or_else(|| {
                BpError::Identity(
                    "Identity is passphrase-protected — provide --passphrase or set BP_PASSPHRASE"
                        .into(),
                )
            })?;
            let json = std::fs::read_to_string(&enc_path).map_err(BpError::Io)?;
            let enc: EncryptedKeyFile = serde_json::from_str(&json)?;
            decrypt_keypair_bytes(&enc, pass)?
        } else if plain_path.exists() {
            // Plaintext identity (legacy / no-passphrase).
            std::fs::read(&plain_path).map_err(BpError::Io)?
        } else {
            return Err(BpError::NotAuthenticated);
        };

        let keypair = Keypair::from_protobuf_encoding(&key_bytes)
            .map_err(|e| BpError::Identity(e.to_string()))?;
        let peer_id = libp2p::PeerId::from_public_key(&keypair.public());
        let fp = fingerprint(&keypair);

        let profile: UserProfile = {
            let profile_path = config::profile_path()?;
            if profile_path.exists() {
                let json = std::fs::read_to_string(&profile_path).map_err(BpError::Io)?;
                serde_json::from_str(&json)?
            } else {
                UserProfile {
                    fingerprint: fp.clone(),
                    alias: None,
                    created_at: chrono::Utc::now(),
                }
            }
        };

        Ok(Self {
            keypair,
            peer_id,
            fingerprint: fp,
            profile,
        })
    }

    /// Remove identity from disk (logout).  Removes both plaintext and
    /// encrypted key files if present, plus the profile.
    pub fn remove() -> BpResult<()> {
        let key_path = config::identity_path()?;
        if key_path.exists() {
            std::fs::remove_file(&key_path).map_err(BpError::Io)?;
        }
        let enc_path = config::encrypted_identity_path()?;
        if enc_path.exists() {
            std::fs::remove_file(&enc_path).map_err(BpError::Io)?;
        }
        let profile_path = config::profile_path()?;
        if profile_path.exists() {
            std::fs::remove_file(&profile_path).map_err(BpError::Io)?;
        }
        Ok(())
    }

    /// Returns `true` if any identity (plaintext or encrypted) is stored on disk.
    pub fn exists() -> BpResult<bool> {
        Ok(config::identity_path()?.exists() || config::encrypted_identity_path()?.exists())
    }
}
