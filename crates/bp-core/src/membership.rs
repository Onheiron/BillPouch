use crate::{
    error::{BpError, BpResult},
    identity::{parse_user_key, DeviceCert, Identity},
    storage::ChunkCipher,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine as _};
use libp2p::identity::{Keypair, PublicKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedKey {
    pub ephemeral: String,
    pub ciphertext: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryBody {
    pub network: String,
    pub genesis: Option<String>,
    pub author: String,
    pub member: String,
    pub wrapped_key: WrappedKey,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub body: EntryBody,
    pub signature: String,
}

impl Entry {
    fn sign(author: &Keypair, body: EntryBody) -> BpResult<Self> {
        let signature = author
            .sign(&serde_json::to_vec(&body)?)
            .map_err(|e| BpError::Identity(format!("cannot sign registry entry: {e}")))?;
        Ok(Self {
            body,
            signature: hex::encode(signature),
        })
    }

    pub fn id(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).unwrap_or_default())
            .to_hex()
            .to_string()
    }

    pub fn signed_by_author(&self) -> Option<String> {
        let (author, fingerprint) = parse_user_key(&self.body.author).ok()?;
        let signature = hex::decode(&self.signature).ok()?;
        let body = serde_json::to_vec(&self.body).ok()?;
        author.verify(&body, &signature).then_some(fingerprint)
    }
}

impl Entry {
    pub fn unwrap_key(&self, identity: &Identity) -> BpResult<[u8; 32]> {
        if parse_user_key(&self.body.member)?.1 != identity.fingerprint {
            return Err(BpError::Identity("this entry is not for you".into()));
        }
        unwrap(&self.body.wrapped_key, &identity.user)
    }
}

pub fn genesis(identity: &Identity, network: &str) -> BpResult<(Entry, [u8; 32])> {
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    let entry = Entry::sign(
        &identity.user,
        EntryBody {
            network: network.to_string(),
            genesis: None,
            author: identity.user_key.clone(),
            member: identity.user_key.clone(),
            wrapped_key: wrap(&key, &identity.user.public())?,
            at: now(),
        },
    )?;
    Ok((entry, key))
}

pub fn invite_entry(
    identity: &Identity,
    genesis: &Entry,
    key: &[u8; 32],
    member_key: &str,
) -> BpResult<Entry> {
    let (member_public, _) = parse_user_key(member_key)?;
    Entry::sign(
        &identity.user,
        EntryBody {
            network: genesis.body.network.clone(),
            genesis: Some(genesis.id()),
            author: identity.user_key.clone(),
            member: member_key.trim().to_string(),
            wrapped_key: wrap(key, &member_public)?,
            at: now(),
        },
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub genesis: Entry,
    pub creator_device: DeviceCert,
    pub entry: Entry,
}

impl Invite {
    pub fn to_token(&self) -> BpResult<String> {
        Ok(B64.encode(serde_json::to_vec(self)?))
    }

    pub fn from_token(token: &str) -> BpResult<Self> {
        let bytes = B64
            .decode(token.trim())
            .map_err(|_| BpError::InvalidInput("malformed invite token".into()))?;
        let invite: Self = serde_json::from_slice(&bytes)?;
        let genesis_ok = invite.genesis.body.genesis.is_none()
            && invite.genesis.body.author == invite.genesis.body.member
            && invite.genesis.signed_by_author().is_some();
        let entry_ok = invite.entry.body.genesis.as_deref() == Some(&invite.genesis.id())
            && invite.entry.signed_by_author().is_some();
        if !genesis_ok || !entry_ok {
            return Err(BpError::InvalidInput("forged invite token".into()));
        }
        Ok(invite)
    }
}

pub fn gossip_topic(key: &[u8; 32]) -> String {
    blake3::keyed_hash(key, b"billpouch/topic/v1").to_hex()[..32].to_string()
}

pub fn seal(key: &[u8; 32], plaintext: &[u8]) -> BpResult<Vec<u8>> {
    ChunkCipher::from_raw_key(*key).encrypt(plaintext)
}

pub fn open(key: &[u8; 32], blob: &[u8]) -> BpResult<Vec<u8>> {
    ChunkCipher::from_raw_key(*key).decrypt(blob)
}

fn wrap(key: &[u8; 32], recipient: &PublicKey) -> BpResult<WrappedKey> {
    let recipient = x25519_public(recipient)?;
    let ephemeral = x25519_dalek::StaticSecret::random_from_rng(rand::thread_rng());
    let ephemeral_public = x25519_dalek::PublicKey::from(&ephemeral);
    let wrapping = wrapping_key(
        ephemeral.diffie_hellman(&recipient).as_bytes(),
        &ephemeral_public,
        &recipient,
    );
    Ok(WrappedKey {
        ephemeral: hex::encode(ephemeral_public.as_bytes()),
        ciphertext: hex::encode(seal(&wrapping, key)?),
    })
}

fn unwrap(wrapped: &WrappedKey, recipient: &Keypair) -> BpResult<[u8; 32]> {
    let ed = recipient
        .clone()
        .try_into_ed25519()
        .map_err(|_| BpError::Identity("user key is not ed25519".into()))?;
    let secret = x25519_dalek::StaticSecret::from(x25519_secret(ed.secret().as_ref()));
    let ephemeral: [u8; 32] = hex::decode(&wrapped.ephemeral)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| BpError::Identity("bad wrapped key".into()))?;
    let ephemeral = x25519_dalek::PublicKey::from(ephemeral);
    let own_public = x25519_dalek::PublicKey::from(&secret);
    let wrapping = wrapping_key(
        secret.diffie_hellman(&ephemeral).as_bytes(),
        &ephemeral,
        &own_public,
    );
    let ciphertext = hex::decode(&wrapped.ciphertext)
        .map_err(|_| BpError::Identity("bad wrapped key".into()))?;
    open(&wrapping, &ciphertext)?
        .try_into()
        .map_err(|_| BpError::Identity("bad wrapped key length".into()))
}

fn wrapping_key(
    shared: &[u8; 32],
    ephemeral: &x25519_dalek::PublicKey,
    recipient: &x25519_dalek::PublicKey,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key("billpouch/wrap/v1");
    hasher.update(shared);
    hasher.update(ephemeral.as_bytes());
    hasher.update(recipient.as_bytes());
    *hasher.finalize().as_bytes()
}

fn x25519_secret(ed25519_seed: &[u8]) -> [u8; 32] {
    let digest = Sha512::digest(ed25519_seed);
    let mut scalar = [0u8; 32];
    scalar.copy_from_slice(&digest[..32]);
    scalar
}

fn x25519_public(public: &PublicKey) -> BpResult<x25519_dalek::PublicKey> {
    let ed = public
        .clone()
        .try_into_ed25519()
        .map_err(|_| BpError::Identity("member key is not ed25519".into()))?;
    let point = curve25519_dalek::edwards::CompressedEdwardsY(ed.to_bytes())
        .decompress()
        .ok_or_else(|| BpError::Identity("invalid ed25519 point".into()))?;
    Ok(x25519_dalek::PublicKey::from(
        point.to_montgomery().to_bytes(),
    ))
}

fn now() -> u64 {
    chrono::Utc::now().timestamp() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_token_carries_a_key_only_the_invitee_can_open() {
        let carlo = Identity::in_memory(Some("carlo".into())).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let (root, key) = genesis(&carlo, "amici").unwrap();
        assert_eq!(root.unwrap_key(&carlo).unwrap(), key);
        let invite = Invite {
            entry: invite_entry(&carlo, &root, &key, &marco.user_key).unwrap(),
            genesis: root,
            creator_device: carlo.cert.clone(),
        };
        let received = Invite::from_token(&invite.to_token().unwrap()).unwrap();
        assert_eq!(received.entry.unwrap_key(&marco).unwrap(), key);
        assert!(received.entry.unwrap_key(&carlo).is_err());
    }

    #[test]
    fn tampered_invites_are_rejected() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let (root, key) = genesis(&carlo, "amici").unwrap();
        let mut invite = Invite {
            entry: invite_entry(&carlo, &root, &key, &marco.user_key).unwrap(),
            genesis: root,
            creator_device: carlo.cert.clone(),
        };
        invite.entry.body.member = Identity::in_memory(None).unwrap().user_key;
        assert!(Invite::from_token(&invite.to_token().unwrap()).is_err());
        let (other, _) = genesis(&carlo, "altri").unwrap();
        invite.entry = invite_entry(&carlo, &other, &key, &marco.user_key).unwrap();
        assert!(Invite::from_token(&invite.to_token().unwrap()).is_err());
    }

    #[test]
    fn gossip_topic_and_sealing_depend_on_the_key() {
        let carlo = Identity::in_memory(None).unwrap();
        let (_, key) = genesis(&carlo, "amici").unwrap();
        let (_, other) = genesis(&carlo, "amici").unwrap();
        assert_ne!(gossip_topic(&key), gossip_topic(&other));
        let blob = seal(&key, b"hello").unwrap();
        assert_eq!(open(&key, &blob).unwrap(), b"hello");
        assert!(open(&other, &blob).is_err());
    }
}
