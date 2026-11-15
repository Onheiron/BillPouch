use crate::{
    error::{BpError, BpResult},
    identity::{member_key, parse_user_key, DeviceCert, Identity},
    storage::ChunkCipher,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine as _};
use libp2p::identity::{Keypair, PublicKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedKey {
    #[serde(with = "crate::wire::hex_bytes")]
    pub ephemeral: String,
    #[serde(with = "crate::wire::hex_bytes")]
    pub ciphertext: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryBody {
    pub network: String,
    pub genesis: Option<String>,
    pub author: String,
    pub member: String,
    pub wrapped_key: WrappedKey,
    #[serde(with = "crate::wire::hex_bytes")]
    pub commitment: String,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub body: EntryBody,
    #[serde(with = "crate::wire::hex_bytes")]
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
    pub fn member(&self) -> BpResult<crate::identity::MemberKey> {
        member_key(&self.body.member)
    }

    pub fn unwrap_key(&self, identity: &Identity) -> BpResult<[u8; 32]> {
        if parse_user_key(&self.body.member)?.1 != identity.key_fingerprint {
            return Err(BpError::Identity("this entry is not for you".into()));
        }
        open_key(&self.body.wrapped_key, &self.body.commitment, identity)
    }
}

pub fn new_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

pub fn key_commitment(key: &[u8; 32]) -> String {
    hex::encode(blake3::derive_key("billpouch/key-commitment/v1", key))
}

pub fn wrap_for(key: &[u8; 32], member_key: &str) -> BpResult<WrappedKey> {
    wrap(key, &parse_user_key(member_key)?.0)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Family {
    #[serde(with = "crate::wire::hex_bytes")]
    pub public: String,
    pub envelopes: BTreeMap<String, WrappedKey>,
    #[serde(with = "crate::wire::hex_bytes")]
    pub history: String,
}

impl Family {
    pub fn seal<'a>(
        keyring: &[[u8; 32]],
        keys: impl IntoIterator<Item = (&'a String, &'a String)>,
    ) -> BpResult<Self> {
        let (current, older) = keyring
            .split_first()
            .ok_or_else(|| BpError::Identity("an empty keyring".into()))?;
        let envelopes = keys
            .into_iter()
            .map(|(fingerprint, line)| Ok((fingerprint.clone(), wrap_for(current, line)?)))
            .collect::<BpResult<_>>()?;
        let history = if older.is_empty() {
            String::new()
        } else {
            hex::encode(seal(current, &older.concat())?)
        };
        Ok(Self {
            public: hex::encode(family_public(current).as_bytes()),
            envelopes,
            history,
        })
    }

    pub fn open(&self, identity: &Identity) -> BpResult<Vec<[u8; 32]>> {
        let envelope = self
            .envelopes
            .get(&identity.key_fingerprint)
            .ok_or_else(|| BpError::Identity("the family key is not open to this device".into()))?;
        let current = unwrap(envelope, &identity.user)?;
        if hex::encode(family_public(&current).as_bytes()) != self.public {
            return Err(BpError::Identity(
                "the family key does not match its public key".into(),
            ));
        }
        let mut keyring = vec![current];
        if !self.history.is_empty() {
            let blob = hex::decode(&self.history)
                .map_err(|_| BpError::Identity("bad family history".into()))?;
            keyring.extend_from_slice(open(&current, &blob)?.as_chunks::<32>().0);
        }
        Ok(keyring)
    }

    pub fn wrap(&self, key: &[u8; 32]) -> BpResult<WrappedKey> {
        let bytes: [u8; 32] = hex::decode(&self.public)
            .ok()
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| BpError::Identity("bad family public key".into()))?;
        wrap_to(key, &x25519_dalek::PublicKey::from(bytes))
    }
}

fn family_public(secret: &[u8; 32]) -> x25519_dalek::PublicKey {
    x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(*secret))
}

pub fn open_with(wrapped: &WrappedKey, keyring: &[[u8; 32]]) -> BpResult<[u8; 32]> {
    keyring
        .iter()
        .find_map(|secret| unwrap_x(wrapped, &x25519_dalek::StaticSecret::from(*secret)).ok())
        .ok_or_else(|| BpError::Identity("none of your family keys opens this".into()))
}

pub fn open_key(wrapped: &WrappedKey, commitment: &str, identity: &Identity) -> BpResult<[u8; 32]> {
    let key = unwrap(wrapped, &identity.user)?;
    if key_commitment(&key) != commitment {
        return Err(BpError::Identity(
            "wrapped key does not match its commitment".into(),
        ));
    }
    Ok(key)
}

pub fn genesis(identity: &Identity, network: &str) -> BpResult<(Entry, [u8; 32])> {
    let key = new_key();
    let entry = Entry::sign(
        &identity.user,
        EntryBody {
            network: network.to_string(),
            genesis: None,
            author: identity.user_key.clone(),
            member: identity.member_line(),
            wrapped_key: wrap(&key, &identity.user.public())?,
            commitment: key_commitment(&key),
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
            commitment: key_commitment(key),
            at: now(),
        },
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub genesis: Entry,
    pub creator_device: DeviceCert,
    pub entry: Entry,
    pub addrs: Vec<String>,
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
            && invite
                .genesis
                .member()
                .is_ok_and(|m| Some(m.key) == invite.genesis.signed_by_author());
        let entry_ok = invite.entry == invite.genesis
            || (invite.entry.body.genesis.as_deref() == Some(&invite.genesis.id())
                && invite.entry.signed_by_author().is_some());
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
    wrap_to(key, &x25519_public(recipient)?)
}

fn wrap_to(key: &[u8; 32], recipient: &x25519_dalek::PublicKey) -> BpResult<WrappedKey> {
    let ephemeral = x25519_dalek::StaticSecret::random_from_rng(rand::thread_rng());
    let ephemeral_public = x25519_dalek::PublicKey::from(&ephemeral);
    let wrapping = wrapping_key(
        ephemeral.diffie_hellman(recipient).as_bytes(),
        &ephemeral_public,
        recipient,
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
    unwrap_x(wrapped, &secret)
}

fn unwrap_x(wrapped: &WrappedKey, secret: &x25519_dalek::StaticSecret) -> BpResult<[u8; 32]> {
    let ephemeral: [u8; 32] = hex::decode(&wrapped.ephemeral)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| BpError::Identity("bad wrapped key".into()))?;
    let ephemeral = x25519_dalek::PublicKey::from(ephemeral);
    let own_public = x25519_dalek::PublicKey::from(secret);
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
            entry: invite_entry(&carlo, &root, &key, &marco.member_line()).unwrap(),
            genesis: root,
            creator_device: carlo.cert.clone(),
            addrs: vec![],
        };
        let received = Invite::from_token(&invite.to_token().unwrap()).unwrap();
        assert_eq!(received.entry.unwrap_key(&marco).unwrap(), key);
        assert!(received.entry.unwrap_key(&carlo).is_err());
    }

    #[test]
    fn the_creator_can_hand_its_genesis_to_another_device() {
        let carlo = Identity::in_memory(None).unwrap();
        let (root, key) = genesis(&carlo, "amici").unwrap();
        let invite = Invite {
            entry: root.clone(),
            genesis: root,
            creator_device: carlo.cert.clone(),
            addrs: vec![],
        };
        let received = Invite::from_token(&invite.to_token().unwrap()).unwrap();
        assert_eq!(received.entry.unwrap_key(&carlo).unwrap(), key);
    }

    #[test]
    fn tampered_invites_are_rejected() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let (root, key) = genesis(&carlo, "amici").unwrap();
        let mut invite = Invite {
            entry: invite_entry(&carlo, &root, &key, &marco.member_line()).unwrap(),
            genesis: root,
            creator_device: carlo.cert.clone(),
            addrs: vec![],
        };
        invite.entry.body.member = Identity::in_memory(None).unwrap().user_key;
        assert!(Invite::from_token(&invite.to_token().unwrap()).is_err());
        let (other, _) = genesis(&carlo, "altri").unwrap();
        invite.entry = invite_entry(&carlo, &other, &key, &marco.member_line()).unwrap();
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

    #[test]
    fn every_key_of_the_family_opens_the_whole_keyring() {
        let laptop = Identity::in_memory(None).unwrap();
        let mut phone = Identity::in_memory(None).unwrap();
        phone.fingerprint = laptop.fingerprint.clone();
        let stranger = Identity::in_memory(None).unwrap();
        let keys = BTreeMap::from([
            (laptop.key_fingerprint.clone(), laptop.user_key.clone()),
            (phone.key_fingerprint.clone(), phone.user_key.clone()),
        ]);
        let first = new_key();
        let family = Family::seal(&[first], &keys).unwrap();
        let file_key = new_key();
        let wrapped = family.wrap(&file_key).unwrap();
        assert_eq!(
            open_with(&wrapped, &family.open(&phone).unwrap()).unwrap(),
            file_key
        );
        assert!(family.open(&stranger).is_err());

        let rotated = Family::seal(
            &[new_key(), first],
            [(&laptop.key_fingerprint, &laptop.user_key)],
        )
        .unwrap();
        assert!(rotated.open(&phone).is_err());
        let keyring = rotated.open(&laptop).unwrap();
        assert_eq!(keyring.len(), 2);
        assert_eq!(open_with(&wrapped, &keyring).unwrap(), file_key);
        assert!(open_with(&rotated.wrap(&file_key).unwrap(), &[first]).is_err());
    }
}
