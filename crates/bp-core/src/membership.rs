use crate::{
    config,
    error::{BpError, BpResult},
    identity::{parse_user_key, Identity},
    storage::ChunkCipher,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine as _};
use libp2p::identity::{Keypair, PublicKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, RwLock};

pub type Memberships = Arc<RwLock<HashMap<String, Registry>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Invite,
}

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

    fn signed_by_author(&self) -> Option<String> {
        let (author, fingerprint) = parse_user_key(&self.body.author).ok()?;
        let signature = hex::decode(&self.signature).ok()?;
        let body = serde_json::to_vec(&self.body).ok()?;
        author.verify(&body, &signature).then_some(fingerprint)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    entries: BTreeMap<String, Entry>,
}

impl Registry {
    pub fn create(identity: &Identity, network: &str) -> BpResult<(Self, [u8; 32])> {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        let genesis = Entry::sign(
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
        let mut entries = BTreeMap::new();
        entries.insert(genesis.id(), genesis);
        Ok((Self { entries }, key))
    }

    pub fn from_entries(entries: impl IntoIterator<Item = Entry>) -> BpResult<Self> {
        let mut registry = Self {
            entries: BTreeMap::new(),
        };
        for entry in entries {
            registry.entries.insert(entry.id(), entry);
        }
        registry.genesis()?;
        Ok(registry)
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    pub fn genesis(&self) -> BpResult<&Entry> {
        let mut roots = self.entries.values().filter(|e| {
            e.body.genesis.is_none()
                && e.body.author == e.body.member
                && e.signed_by_author().is_some()
        });
        match (roots.next(), roots.next()) {
            (Some(root), None) => Ok(root),
            _ => Err(BpError::Identity(
                "registry must have exactly one valid genesis".into(),
            )),
        }
    }

    pub fn id(&self) -> BpResult<String> {
        Ok(self.genesis()?.id())
    }

    pub fn name(&self) -> BpResult<&str> {
        Ok(&self.genesis()?.body.network)
    }

    pub fn creator(&self) -> BpResult<String> {
        Ok(parse_user_key(&self.genesis()?.body.author)?.1)
    }

    pub fn members(&self) -> BpResult<HashSet<String>> {
        Ok(self.valid_entries()?.into_keys().collect())
    }

    pub fn is_member(&self, fingerprint: &str) -> bool {
        self.members()
            .map(|m| m.contains(fingerprint))
            .unwrap_or(false)
    }

    pub fn has_capability(&self, fingerprint: &str, capability: Capability) -> bool {
        match capability {
            Capability::Invite => self.creator().is_ok_and(|c| c == fingerprint),
        }
    }

    fn valid_entries(&self) -> BpResult<HashMap<String, &Entry>> {
        let genesis = self.genesis()?;
        let genesis_id = genesis.id();
        let creator = self.creator()?;
        let mut members: HashMap<String, &Entry> = HashMap::new();
        members.insert(creator, genesis);
        for entry in self.entries.values() {
            if entry.body.genesis.as_deref() != Some(&genesis_id) {
                continue;
            }
            let Some(author) = entry.signed_by_author() else {
                continue;
            };
            let Ok((_, member)) = parse_user_key(&entry.body.member) else {
                continue;
            };
            if members.contains_key(&author)
                && self.has_capability(&author, Capability::Invite)
                && !members.contains_key(&member)
            {
                members.insert(member, entry);
            }
        }
        Ok(members)
    }

    pub fn invite(&mut self, identity: &Identity, member_key: &str) -> BpResult<()> {
        let (member_public, member) = parse_user_key(member_key)?;
        if !self.has_capability(&identity.fingerprint, Capability::Invite) {
            return Err(BpError::Identity(
                "you do not have the invite capability in this network".into(),
            ));
        }
        if self.is_member(&member) {
            return Err(BpError::Identity(format!("{member} is already a member")));
        }
        let key = self.network_key(identity)?;
        let entry = Entry::sign(
            &identity.user,
            EntryBody {
                network: self.name()?.to_string(),
                genesis: Some(self.id()?),
                author: identity.user_key.clone(),
                member: member_key.trim().to_string(),
                wrapped_key: wrap(&key, &member_public)?,
                at: now(),
            },
        )?;
        self.entries.insert(entry.id(), entry);
        Ok(())
    }

    pub fn merge(&mut self, other: &Registry) -> BpResult<bool> {
        if other.id()? != self.id()? {
            return Err(BpError::Identity(
                "cannot merge registries of different networks".into(),
            ));
        }
        let before = self.entries.len();
        for (id, entry) in &other.entries {
            self.entries
                .entry(id.clone())
                .or_insert_with(|| entry.clone());
        }
        Ok(self.entries.len() > before)
    }

    pub fn network_key(&self, identity: &Identity) -> BpResult<[u8; 32]> {
        let entries = self.valid_entries()?;
        let own = entries
            .get(&identity.fingerprint)
            .ok_or_else(|| BpError::Identity("you are not a member of this network".into()))?;
        unwrap(&own.body.wrapped_key, &identity.user)
    }

    pub fn to_token(&self) -> BpResult<String> {
        Ok(B64.encode(serde_json::to_vec(self)?))
    }

    pub fn from_token(token: &str) -> BpResult<Self> {
        let bytes = B64
            .decode(token.trim())
            .map_err(|_| BpError::InvalidInput("malformed invite token".into()))?;
        let registry: Self = serde_json::from_slice(&bytes)?;
        registry.genesis()?;
        Ok(registry)
    }

    pub fn save(&self) -> BpResult<()> {
        let dir = config::networks_dir()?;
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join(format!("{}.json", self.name()?)),
            serde_json::to_string_pretty(self)?,
        )?;
        Ok(())
    }

    pub fn load_all() -> BpResult<HashMap<String, Registry>> {
        let dir = config::networks_dir()?;
        let mut all = HashMap::new();
        if !dir.exists() {
            return Ok(all);
        }
        for entry in std::fs::read_dir(dir)? {
            let registry: Registry =
                serde_json::from_str(&std::fs::read_to_string(entry?.path())?)?;
            all.insert(registry.name()?.to_string(), registry);
        }
        Ok(all)
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

    fn network() -> (Identity, Registry, [u8; 32]) {
        let carlo = Identity::in_memory(Some("carlo".into())).unwrap();
        let (registry, key) = Registry::create(&carlo, "amici").unwrap();
        (carlo, registry, key)
    }

    #[test]
    fn creator_is_the_only_member_and_holds_the_key() {
        let (carlo, registry, key) = network();
        assert_eq!(
            registry.members().unwrap(),
            HashSet::from([carlo.fingerprint.clone()])
        );
        assert_eq!(registry.network_key(&carlo).unwrap(), key);
        assert!(registry.has_capability(&carlo.fingerprint, Capability::Invite));
    }

    #[test]
    fn invited_member_can_unwrap_the_network_key() {
        let (carlo, mut registry, key) = network();
        let marco = Identity::in_memory(None).unwrap();
        registry.invite(&carlo, &marco.user_key).unwrap();
        let received = Registry::from_token(&registry.to_token().unwrap()).unwrap();
        assert!(received.is_member(&marco.fingerprint));
        assert_eq!(received.network_key(&marco).unwrap(), key);
        assert!(received
            .network_key(&Identity::in_memory(None).unwrap())
            .is_err());
    }

    #[test]
    fn only_the_creator_can_invite_until_reputation_snapshots_exist() {
        let (carlo, mut registry, _) = network();
        let marco = Identity::in_memory(None).unwrap();
        let lucia = Identity::in_memory(None).unwrap();
        registry.invite(&carlo, &marco.user_key).unwrap();
        assert!(registry.invite(&marco, &lucia.user_key).is_err());
        assert!(registry.invite(&carlo, &marco.user_key).is_err());
    }

    #[test]
    fn forged_entries_are_ignored() {
        let (carlo, mut registry, _) = network();
        let mallory = Identity::in_memory(None).unwrap();
        let genesis_id = registry.id().unwrap();

        let self_added = Entry::sign(
            &mallory.user,
            EntryBody {
                network: "amici".into(),
                genesis: Some(genesis_id.clone()),
                author: mallory.user_key.clone(),
                member: mallory.user_key.clone(),
                wrapped_key: wrap(&[0; 32], &mallory.user.public()).unwrap(),
                at: 0,
            },
        )
        .unwrap();
        let mut stolen_author = self_added.clone();
        stolen_author.body.author = carlo.user_key.clone();

        registry.entries.insert(self_added.id(), self_added.clone());
        registry.entries.insert(stolen_author.id(), stolen_author);
        assert!(!registry.is_member(&mallory.fingerprint));
    }

    #[test]
    fn a_second_genesis_invalidates_the_registry() {
        let (_, mut registry, _) = network();
        let (_, other, _) = network();
        for entry in other.entries() {
            registry.entries.insert(entry.id(), entry.clone());
        }
        assert!(registry.genesis().is_err());
    }

    #[test]
    fn merge_unions_entries_of_the_same_network_only() {
        let (carlo, registry, _) = network();
        let mut copy = registry.clone();
        copy.invite(&carlo, &Identity::in_memory(None).unwrap().user_key)
            .unwrap();
        let mut original = registry.clone();
        assert!(original.merge(&copy).unwrap());
        assert!(!original.merge(&copy).unwrap());
        assert_eq!(original.members().unwrap().len(), 2);
        assert!(original.merge(&network().1).is_err());
    }

    #[test]
    fn gossip_topic_and_sealing_depend_on_the_key() {
        let (_, _, key) = network();
        let (_, _, other) = network();
        assert_ne!(gossip_topic(&key), gossip_topic(&other));
        let blob = seal(&key, b"hello").unwrap();
        assert_eq!(open(&key, &blob).unwrap(), b"hello");
        assert!(open(&other, &blob).is_err());
    }
}
