use crate::{
    config,
    error::{BpError, BpResult},
};
use libp2p::{
    identity::{ed25519, Keypair, PublicKey},
    PeerId,
};
use serde::{Deserialize, Serialize};
use ssh_key::{
    private::{Ed25519Keypair, KeypairData},
    public::{Ed25519PublicKey, KeyData},
    rand_core::OsRng,
    HashAlg, LineEnding, PrivateKey,
};
use std::path::Path;

const DEVICE_CERT_CONTEXT: &[u8] = b"billpouch/device-cert/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub alias: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCert {
    pub user_key: String,
    pub device_key: String,
    pub issued_at: u64,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedDevice {
    pub fingerprint: String,
    pub peer_id: PeerId,
}

impl DeviceCert {
    pub fn issue(user: &Keypair, user_key: &str, device: &PublicKey) -> BpResult<Self> {
        let device_key = hex::encode(device.encode_protobuf());
        let issued_at = chrono::Utc::now().timestamp() as u64;
        let signature = user
            .sign(&cert_message(user_key, &device_key, issued_at))
            .map_err(|e| BpError::Identity(format!("cannot sign device certificate: {e}")))?;
        Ok(Self {
            user_key: user_key.to_string(),
            device_key,
            issued_at,
            signature: hex::encode(signature),
        })
    }

    pub fn verify(&self) -> BpResult<VerifiedDevice> {
        let (user_public, fingerprint) = parse_user_key(&self.user_key)?;
        let signature = hex::decode(&self.signature)
            .map_err(|_| BpError::Identity("device certificate: bad signature encoding".into()))?;
        let message = cert_message(&self.user_key, &self.device_key, self.issued_at);
        if !user_public.verify(&message, &signature) {
            return Err(BpError::Identity(
                "device certificate: invalid signature".into(),
            ));
        }
        let device_bytes = hex::decode(&self.device_key)
            .map_err(|_| BpError::Identity("device certificate: bad device key".into()))?;
        let device = PublicKey::try_decode_protobuf(&device_bytes)
            .map_err(|e| BpError::Identity(format!("device certificate: {e}")))?;
        Ok(VerifiedDevice {
            fingerprint,
            peer_id: PeerId::from_public_key(&device),
        })
    }
}

fn cert_message(user_key: &str, device_key: &str, issued_at: u64) -> Vec<u8> {
    let mut msg = DEVICE_CERT_CONTEXT.to_vec();
    for field in [user_key.as_bytes(), device_key.as_bytes()] {
        msg.extend_from_slice(&(field.len() as u32).to_le_bytes());
        msg.extend_from_slice(field);
    }
    msg.extend_from_slice(&issued_at.to_le_bytes());
    msg
}

pub fn parse_user_key(openssh: &str) -> BpResult<(PublicKey, String)> {
    let key = ssh_key::PublicKey::from_openssh(openssh)
        .map_err(|e| BpError::Identity(format!("invalid SSH public key: {e}")))?;
    let KeyData::Ed25519(Ed25519PublicKey(bytes)) = key.key_data() else {
        return Err(BpError::Identity(
            "only ssh-ed25519 keys are supported".into(),
        ));
    };
    let public = ed25519::PublicKey::try_from_bytes(bytes)
        .map_err(|e| BpError::Identity(format!("invalid ed25519 key: {e}")))?;
    Ok((
        PublicKey::from(public),
        key.fingerprint(HashAlg::Sha256).to_string(),
    ))
}

#[derive(Clone)]
pub struct Identity {
    pub user: Keypair,
    pub user_key: String,
    pub fingerprint: String,
    pub device: Keypair,
    pub peer_id: PeerId,
    pub cert: DeviceCert,
    pub profile: UserProfile,
}

impl Identity {
    pub fn create(
        ssh_key: Option<&Path>,
        alias: Option<String>,
        passphrase: Option<&str>,
    ) -> BpResult<Self> {
        if Self::exists()? {
            return Err(BpError::Identity(
                "an identity already exists on this machine — run `bp logout` first".into(),
            ));
        }
        config::ensure_dirs()?;

        let openssh = match ssh_key {
            Some(path) => {
                let text = std::fs::read_to_string(path)?;
                unlock(&text, passphrase)?;
                text
            }
            None => {
                let comment = alias.clone().unwrap_or_else(|| "billpouch".into());
                let mut key =
                    PrivateKey::random(&mut OsRng, ssh_key::Algorithm::Ed25519).map_err(ssh_err)?;
                key.set_comment(comment);
                if let Some(pass) = passphrase {
                    key = key.encrypt(&mut OsRng, pass).map_err(ssh_err)?;
                }
                key.to_openssh(LineEnding::LF).map_err(ssh_err)?.to_string()
            }
        };
        write_private(&config::user_key_path()?, openssh.as_bytes())?;

        let (user, user_key) = unlock(&openssh, passphrase)?;
        let device = Keypair::generate_ed25519();
        let device_bytes = device
            .to_protobuf_encoding()
            .map_err(|e| BpError::Identity(e.to_string()))?;
        write_private(&config::device_key_path()?, &device_bytes)?;

        let cert = DeviceCert::issue(&user, &user_key, &device.public())?;
        std::fs::write(
            config::device_cert_path()?,
            serde_json::to_string_pretty(&cert)?,
        )?;

        let profile = UserProfile {
            alias,
            created_at: chrono::Utc::now(),
        };
        std::fs::write(
            config::profile_path()?,
            serde_json::to_string_pretty(&profile)?,
        )?;

        Self::assemble(user, user_key, device, cert, profile)
    }

    pub fn load(passphrase: Option<&str>) -> BpResult<Self> {
        let key_path = config::user_key_path()?;
        if !key_path.exists() {
            return Err(BpError::NotAuthenticated);
        }
        let (user, user_key) = unlock(&std::fs::read_to_string(key_path)?, passphrase)?;
        let device = Keypair::from_protobuf_encoding(&std::fs::read(config::device_key_path()?)?)
            .map_err(|e| BpError::Identity(format!("device key: {e}")))?;
        let cert: DeviceCert =
            serde_json::from_str(&std::fs::read_to_string(config::device_cert_path()?)?)?;
        let profile: UserProfile =
            serde_json::from_str(&std::fs::read_to_string(config::profile_path()?)?)?;
        Self::assemble(user, user_key, device, cert, profile)
    }

    pub fn in_memory(alias: Option<String>) -> BpResult<Self> {
        let key = PrivateKey::random(&mut OsRng, ssh_key::Algorithm::Ed25519).map_err(ssh_err)?;
        let (user, user_key) = unlock(&key.to_openssh(LineEnding::LF).map_err(ssh_err)?, None)?;
        let device = Keypair::generate_ed25519();
        let cert = DeviceCert::issue(&user, &user_key, &device.public())?;
        let profile = UserProfile {
            alias,
            created_at: chrono::Utc::now(),
        };
        Self::assemble(user, user_key, device, cert, profile)
    }

    fn assemble(
        user: Keypair,
        user_key: String,
        device: Keypair,
        cert: DeviceCert,
        profile: UserProfile,
    ) -> BpResult<Self> {
        let verified = cert.verify()?;
        let peer_id = PeerId::from_public_key(&device.public());
        if cert.user_key != user_key || verified.peer_id != peer_id {
            return Err(BpError::Identity(
                "device certificate does not match this user and device".into(),
            ));
        }
        Ok(Self {
            user,
            user_key,
            fingerprint: verified.fingerprint,
            device,
            peer_id,
            cert,
            profile,
        })
    }

    pub fn remove() -> BpResult<()> {
        for path in [
            config::user_key_path()?,
            config::device_key_path()?,
            config::device_cert_path()?,
            config::profile_path()?,
        ] {
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    pub fn exists() -> BpResult<bool> {
        Ok(config::user_key_path()?.exists())
    }

    pub fn secret_material(&self) -> [u8; 32] {
        let raw = self.user.to_protobuf_encoding().unwrap_or_default();
        *blake3::Hasher::new()
            .update(b"billpouch/secret-material/v2")
            .update(&raw)
            .finalize()
            .as_bytes()
    }
}

fn unlock(openssh: &str, passphrase: Option<&str>) -> BpResult<(Keypair, String)> {
    let mut key = PrivateKey::from_openssh(openssh)
        .map_err(|e| BpError::Identity(format!("invalid OpenSSH private key: {e}")))?;
    if key.is_encrypted() {
        let pass = passphrase.ok_or_else(|| {
            BpError::Identity(
                "the SSH key is passphrase-protected — provide --passphrase or set BP_PASSPHRASE"
                    .into(),
            )
        })?;
        key = key
            .decrypt(pass)
            .map_err(|_| BpError::Identity("wrong passphrase for the SSH key".into()))?;
    }
    let KeypairData::Ed25519(Ed25519Keypair { private, .. }) = key.key_data() else {
        return Err(BpError::Identity(
            "only ssh-ed25519 keys are supported".into(),
        ));
    };
    let secret = ed25519::SecretKey::try_from_bytes(private.to_bytes())
        .map_err(|e| BpError::Identity(format!("invalid ed25519 key: {e}")))?;
    let user = Keypair::from(ed25519::Keypair::from(secret));
    let public = key.public_key().to_openssh().map_err(ssh_err)?;
    Ok((user, public))
}

fn write_private(path: &Path, bytes: &[u8]) -> BpResult<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn ssh_err(e: ssh_key::Error) -> BpError {
    BpError::Identity(format!("SSH key error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_identity_has_valid_certificate() {
        let id = Identity::in_memory(Some("carlo".into())).unwrap();
        let verified = id.cert.verify().unwrap();
        assert_eq!(verified.peer_id, id.peer_id);
        assert_eq!(verified.fingerprint, id.fingerprint);
        assert!(id.fingerprint.starts_with("SHA256:"));
        assert!(id.user_key.starts_with("ssh-ed25519 "));
    }

    #[test]
    fn device_key_differs_from_user_key() {
        let id = Identity::in_memory(None).unwrap();
        assert_ne!(id.peer_id, PeerId::from_public_key(&id.user.public()));
    }

    #[test]
    fn tampered_certificate_is_rejected() {
        let id = Identity::in_memory(None).unwrap();
        let other = Identity::in_memory(None).unwrap();

        let mut swapped_device = id.cert.clone();
        swapped_device.device_key = other.cert.device_key.clone();
        assert!(swapped_device.verify().is_err());

        let mut swapped_user = id.cert.clone();
        swapped_user.user_key = other.user_key.clone();
        assert!(swapped_user.verify().is_err());
    }

    #[test]
    fn two_devices_of_the_same_user_share_fingerprint_not_peer_id() {
        let key = PrivateKey::random(&mut OsRng, ssh_key::Algorithm::Ed25519).unwrap();
        let openssh = key.to_openssh(LineEnding::LF).unwrap();
        let (user, user_key) = unlock(&openssh, None).unwrap();
        let laptop = Keypair::generate_ed25519();
        let phone = Keypair::generate_ed25519();
        let a = DeviceCert::issue(&user, &user_key, &laptop.public())
            .unwrap()
            .verify()
            .unwrap();
        let b = DeviceCert::issue(&user, &user_key, &phone.public())
            .unwrap()
            .verify()
            .unwrap();
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_ne!(a.peer_id, b.peer_id);
    }

    #[test]
    fn encrypted_ssh_key_needs_the_right_passphrase() {
        let key = PrivateKey::random(&mut OsRng, ssh_key::Algorithm::Ed25519)
            .unwrap()
            .encrypt(&mut OsRng, "s3cret")
            .unwrap();
        let openssh = key.to_openssh(LineEnding::LF).unwrap();
        assert!(unlock(&openssh, None).is_err());
        assert!(unlock(&openssh, Some("wrong")).is_err());
        assert!(unlock(&openssh, Some("s3cret")).is_ok());
    }

    #[test]
    fn fingerprint_matches_ssh_keygen_format() {
        let id = Identity::in_memory(None).unwrap();
        let (_, fp) = parse_user_key(&id.user_key).unwrap();
        assert_eq!(fp, id.fingerprint);
        assert_eq!(fp.len(), "SHA256:".len() + 43);
    }
}
