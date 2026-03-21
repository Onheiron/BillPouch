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
use argon2::Argon2;
use blake3;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
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
    fingerprint_pubkey(&keypair.public())
}

/// Fingerprint of a public key (hex-encoded SHA-256[0..8]).
///
/// Used to verify invite token signatures without a pre-existing trust anchor.
pub fn fingerprint_pubkey(pubkey: &libp2p::identity::PublicKey) -> String {
    let pub_bytes = pubkey.encode_protobuf();
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

    /// Derive 32 bytes of secret material from the keypair.
    ///
    /// Used as the master input for per-file Content Encryption Key (CEK)
    /// derivation.  The output is deterministic for a given keypair but must
    /// **never** be stored or logged — treat it like the private key itself.
    pub fn secret_material(&self) -> [u8; 32] {
        let raw = self.keypair.to_protobuf_encoding().unwrap_or_default();
        *blake3::Hasher::new()
            .update(b"billpouch/secret-material/v1")
            .update(&raw)
            .finalize()
            .as_bytes()
    }

    /// Export this identity to a portable JSON file at `dest`.
    ///
    /// The on-disk key files are copied verbatim: passphrase-protected
    /// identities export the encrypted form (no passphrase needed here);
    /// plaintext identities hex-encode the raw protobuf bytes.
    /// The output file is safe to transfer to another machine; use
    /// [`Identity::import_from_file`] on the receiving end.
    pub fn export_to_file(dest: &std::path::Path) -> BpResult<()> {
        let enc_path = config::encrypted_identity_path()?;
        let plain_path = config::identity_path()?;

        let key = if enc_path.exists() {
            let json = std::fs::read_to_string(&enc_path).map_err(BpError::Io)?;
            let enc: EncryptedKeyFile = serde_json::from_str(&json)?;
            ExportedKeyData::Encrypted(enc)
        } else if plain_path.exists() {
            let bytes = std::fs::read(&plain_path).map_err(BpError::Io)?;
            ExportedKeyData::Plaintext {
                key_hex: hex::encode(bytes),
            }
        } else {
            return Err(BpError::NotAuthenticated);
        };

        let profile_path = config::profile_path()?;
        let profile: UserProfile = if profile_path.exists() {
            let json = std::fs::read_to_string(&profile_path).map_err(BpError::Io)?;
            serde_json::from_str(&json)?
        } else {
            return Err(BpError::Config(
                "Profile not found — run `bp login` first".into(),
            ));
        };

        let export = ExportedIdentity {
            version: 1,
            key,
            profile,
            exported_at: chrono::Utc::now(),
        };

        let json = serde_json::to_string_pretty(&export)?;
        std::fs::write(dest, json).map_err(BpError::Io)?;
        Ok(())
    }

    /// Import an identity from a portable export file created by
    /// [`Identity::export_to_file`].
    ///
    /// Installs the keypair and profile to the XDG data directory.
    /// Returns the imported [`UserProfile`] on success.
    ///
    /// If an identity already exists and `overwrite` is `false`, returns
    /// [`BpError::Config`] with a message asking the user to `bp logout` first.
    pub fn import_from_file(src: &std::path::Path, overwrite: bool) -> BpResult<UserProfile> {
        if !overwrite && Identity::exists()? {
            return Err(BpError::Config(
                "An identity already exists on this machine. \
                 Run `bp logout` first, or use --force to overwrite."
                    .into(),
            ));
        }

        config::ensure_dirs()?;

        let json = std::fs::read_to_string(src).map_err(BpError::Io)?;
        let export: ExportedIdentity = serde_json::from_str(&json)
            .map_err(|e| BpError::Config(format!("Invalid identity export file: {e}")))?;

        if export.version != 1 {
            return Err(BpError::Config(format!(
                "Unsupported identity export version: {}",
                export.version
            )));
        }

        // Remove existing files before overwriting.
        if overwrite {
            let _ = Identity::remove();
        }

        match export.key {
            ExportedKeyData::Encrypted(enc) => {
                let json = serde_json::to_string_pretty(&enc)?;
                std::fs::write(config::encrypted_identity_path()?, json).map_err(BpError::Io)?;
            }
            ExportedKeyData::Plaintext { key_hex } => {
                let bytes = hex::decode(&key_hex)
                    .map_err(|e| BpError::Config(format!("Invalid key hex: {e}")))?;
                std::fs::write(config::identity_path()?, bytes).map_err(BpError::Io)?;
            }
        }

        let profile_json = serde_json::to_string_pretty(&export.profile)?;
        std::fs::write(config::profile_path()?, profile_json).map_err(BpError::Io)?;

        tracing::info!(
            fingerprint = %export.profile.fingerprint,
            "Identity imported from {}",
            src.display()
        );

        Ok(export.profile)
    }
}

// ── Portable identity export ──────────────────────────────────────────────────

/// Key data carried inside an [`ExportedIdentity`].
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "lowercase")]
pub enum ExportedKeyData {
    /// Raw protobuf-encoded keypair, hex-encoded (no passphrase).
    Plaintext { key_hex: String },
    /// Argon2id + ChaCha20-Poly1305 encrypted keypair (passphrase still needed to use).
    Encrypted(EncryptedKeyFile),
}

/// Portable JSON bundle that carries a complete BillPouch identity.
///
/// Produced by [`Identity::export_to_file`]; consumed by
/// [`Identity::import_from_file`].  Safe to store on USB, email, etc. —
/// the encrypted variant requires the original passphrase to use, and the
/// plaintext variant should be treated like a private key file.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportedIdentity {
    /// Format version — currently `1`.
    pub version: u8,
    /// The keypair (plaintext or encrypted).
    pub key: ExportedKeyData,
    /// User profile (alias, fingerprint, creation timestamp).
    pub profile: UserProfile,
    /// UTC timestamp when this bundle was created.
    pub exported_at: chrono::DateTime<chrono::Utc>,
}
