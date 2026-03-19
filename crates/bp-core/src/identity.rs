//! User identity: Ed25519 keypair persisted to disk.
//! The PeerId is derived deterministically from the public key.

use crate::{
    config,
    error::{BpError, BpResult},
};
use libp2p::identity::Keypair;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    pub fn generate(alias: Option<String>) -> BpResult<Self> {
        config::ensure_dirs()?;
        let keypair = Keypair::generate_ed25519();
        let peer_id = libp2p::PeerId::from_public_key(&keypair.public());
        let fp = fingerprint(&keypair);

        let profile = UserProfile {
            fingerprint: fp.clone(),
            alias,
            created_at: chrono::Utc::now(),
        };

        // Persist keypair
        let key_bytes = keypair
            .to_protobuf_encoding()
            .map_err(|e| BpError::Identity(e.to_string()))?;
        std::fs::write(config::identity_path()?, &key_bytes).map_err(BpError::Io)?;

        // Persist profile
        let json = serde_json::to_string_pretty(&profile)?;
        std::fs::write(config::profile_path()?, json).map_err(BpError::Io)?;

        tracing::info!("New identity created — fingerprint: {}", fp);
        Ok(Self {
            keypair,
            peer_id,
            fingerprint: fp,
            profile,
        })
    }

    /// Load existing identity from disk. Returns `Err(NotAuthenticated)` if not found.
    pub fn load() -> BpResult<Self> {
        let key_path = config::identity_path()?;
        if !key_path.exists() {
            return Err(BpError::NotAuthenticated);
        }
        let key_bytes = std::fs::read(&key_path).map_err(BpError::Io)?;
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

    /// Remove identity from disk (logout).
    pub fn remove() -> BpResult<()> {
        let key_path = config::identity_path()?;
        if key_path.exists() {
            std::fs::remove_file(&key_path).map_err(BpError::Io)?;
        }
        let profile_path = config::profile_path()?;
        if profile_path.exists() {
            std::fs::remove_file(&profile_path).map_err(BpError::Io)?;
        }
        Ok(())
    }

    /// Returns true if an identity is stored on disk.
    pub fn exists() -> BpResult<bool> {
        Ok(config::identity_path()?.exists())
    }
}
