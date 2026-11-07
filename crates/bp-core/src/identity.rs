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

const ARGON2_SALT_LEN: usize = 16;
const CHACHA_NONCE_LEN: usize = 12;
const CHACHA_KEY_LEN: usize = 32;

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedKeyFile {
    pub version: u8,
    pub salt: String,
    pub nonce: String,
    pub ciphertext: String,
}

fn derive_key(passphrase: &str, salt: &[u8]) -> BpResult<[u8; CHACHA_KEY_LEN]> {
    let mut key = [0u8; CHACHA_KEY_LEN];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| BpError::Identity(format!("KDF error: {e}")))?;
    Ok(key)
}

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

pub fn fingerprint(keypair: &Keypair) -> String {
    fingerprint_pubkey(&keypair.public())
}

pub fn fingerprint_pubkey(pubkey: &libp2p::identity::PublicKey) -> String {
    let pub_bytes = pubkey.encode_protobuf();
    let hash = Sha256::digest(&pub_bytes);
    hex::encode(&hash[..8])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub fingerprint: String,
    pub alias: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone)]
pub struct Identity {
    pub keypair: Keypair,
    pub peer_id: libp2p::PeerId,
    pub fingerprint: String,
    pub profile: UserProfile,
}

impl Identity {
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

        let json = serde_json::to_string_pretty(&profile)?;
        std::fs::write(config::profile_path()?, json).map_err(BpError::Io)?;

        Ok(Self {
            keypair,
            peer_id,
            fingerprint: fp,
            profile,
        })
    }

    pub fn load(passphrase: Option<&str>) -> BpResult<Self> {
        let enc_path = config::encrypted_identity_path()?;
        let plain_path = config::identity_path()?;

        let key_bytes = if enc_path.exists() {
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
            std::fs::read(&plain_path).map_err(BpError::Io)?
        } else {
            return Err(BpError::NotAuthenticated);
        };

        let keypair = Keypair::from_protobuf_encoding(&key_bytes)
            .map_err(|e| BpError::Identity(e.to_string()))?;
        let peer_id = libp2p::PeerId::from_public_key(&keypair.public());
        let fp = fingerprint(&keypair);

        let profile: UserProfile =
            serde_json::from_str(&std::fs::read_to_string(config::profile_path()?)?)?;

        Ok(Self {
            keypair,
            peer_id,
            fingerprint: fp,
            profile,
        })
    }

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

    pub fn exists() -> BpResult<bool> {
        Ok(config::identity_path()?.exists() || config::encrypted_identity_path()?.exists())
    }

    pub fn secret_material(&self) -> [u8; 32] {
        let raw = self.keypair.to_protobuf_encoding().unwrap_or_default();
        *blake3::Hasher::new()
            .update(b"billpouch/secret-material/v1")
            .update(&raw)
            .finalize()
            .as_bytes()
    }

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

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "lowercase")]
pub enum ExportedKeyData {
    Plaintext { key_hex: String },
    Encrypted(EncryptedKeyFile),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportedIdentity {
    pub version: u8,
    pub key: ExportedKeyData,
    pub profile: UserProfile,
    pub exported_at: chrono::DateTime<chrono::Utc>,
}
