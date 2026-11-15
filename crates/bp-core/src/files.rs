use crate::{
    config,
    error::{BpError, BpResult},
    identity::{parse_user_key, Identity},
    ledger::Ledger,
    membership::{new_key, open_with, Family, WrappedKey},
    storage::ChunkCipher,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CHUNK_BYTES: usize = 4 * 1024 * 1024;
pub const MIN_CHUNK_BYTES: usize = 64 * 1024;
pub const ORDER_RESENDS: u32 = 10;

pub fn new_file_id() -> String {
    hex::encode(&new_key()[..8])
}

const RAW: u8 = 0;
const ZSTD: u8 = 1;
const ZSTD_LEVEL: i32 = 3;

pub fn pack(plain: &[u8], compress: bool) -> (Vec<u8>, bool) {
    if compress {
        if let Ok(small) = zstd::bulk::compress(plain, ZSTD_LEVEL) {
            if small.len() * 100 < plain.len() * 97 {
                let mut out = Vec::with_capacity(small.len() + 1);
                out.push(ZSTD);
                out.extend_from_slice(&small);
                return (out, true);
            }
        }
    }
    let mut out = Vec::with_capacity(plain.len() + 1);
    out.push(RAW);
    out.extend_from_slice(plain);
    (out, false)
}

pub fn unpack(packed: &[u8]) -> BpResult<Vec<u8>> {
    match packed.split_first() {
        Some((&RAW, rest)) => Ok(rest.to_vec()),
        Some((&ZSTD, rest)) => zstd::bulk::decompress(rest, CHUNK_BYTES)
            .map_err(|e| BpError::Storage(format!("cannot decompress a chunk: {e}"))),
        _ => Err(BpError::Storage("unknown chunk encoding".into())),
    }
}

pub fn chunk_id(file_id: &str, index: usize) -> String {
    blake3::hash(format!("billpouch/chunk/{file_id}/{index}").as_bytes()).to_hex()[..16].to_string()
}

pub fn chunk_ids(file_id: &str, count: u64) -> impl Iterator<Item = String> + '_ {
    (0..count as usize).map(move |i| chunk_id(file_id, i))
}

fn signed(context: &[u8], value: &impl Serialize) -> Vec<u8> {
    let mut msg = context.to_vec();
    msg.extend_from_slice(&serde_json::to_vec(value).unwrap_or_default());
    msg
}

fn sign(identity: &Identity, message: &[u8]) -> BpResult<String> {
    identity
        .user
        .sign(message)
        .map(hex::encode)
        .map_err(|e| BpError::Identity(format!("cannot sign: {e}")))
}

fn signer(key: &str, message: &[u8], signature: &str) -> Option<String> {
    let (public, fingerprint) = parse_user_key(key).ok()?;
    let signature = hex::decode(signature).ok()?;
    public.verify(message, &signature).then_some(fingerprint)
}

pub trait Keys {
    fn owner_of(&self, key: &str) -> Option<&str>;
    fn ever_owned(&self, user: &str, key: &str) -> bool;
}

impl Keys for Ledger {
    fn owner_of(&self, key: &str) -> Option<&str> {
        self.key_owner(key)
    }

    fn ever_owned(&self, user: &str, key: &str) -> bool {
        self.ever_signed_for(user, key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub file_id: String,
    pub owner: String,
    pub key: String,
    pub grantee: String,
    pub signature: String,
}

impl Grant {
    fn message(file_id: &str, owner: &str, key: &str, grantee: &str) -> Vec<u8> {
        signed(b"billpouch/grant/v3", &(file_id, owner, key, grantee))
    }

    pub fn sign(owner: &Identity, file_id: &str, grantee: &str) -> BpResult<Self> {
        let message = Self::message(file_id, &owner.fingerprint, &owner.user_key, grantee);
        Ok(Self {
            file_id: file_id.to_string(),
            owner: owner.fingerprint.clone(),
            key: owner.user_key.clone(),
            grantee: grantee.to_string(),
            signature: sign(owner, &message)?,
        })
    }

    fn signer(&self) -> Option<String> {
        let message = Self::message(&self.file_id, &self.owner, &self.key, &self.grantee);
        signer(&self.key, &message, &self.signature)
    }

    pub fn allows(&self, file_id: &str, owner: &str, requester: &str, keys: &impl Keys) -> bool {
        self.file_id == file_id
            && self.owner == owner
            && self.grantee == requester
            && self
                .signer()
                .is_some_and(|key| keys.ever_owned(&self.owner, &key))
    }

    pub fn is_owner_claim(&self, keys: &impl Keys) -> bool {
        self.grantee == self.owner
            && self
                .signer()
                .is_some_and(|key| keys.owner_of(&key) == Some(self.owner.as_str()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Delete,
    Revoke { member: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub owner: String,
    pub key: String,
    pub file_id: String,
    pub action: Action,
    pub signature: String,
}

impl Order {
    fn message(owner: &str, key: &str, file_id: &str, action: &Action) -> Vec<u8> {
        signed(b"billpouch/order/v2", &(owner, key, file_id, action))
    }

    pub fn sign(owner: &Identity, file_id: &str, action: Action) -> BpResult<Self> {
        let message = Self::message(&owner.fingerprint, &owner.user_key, file_id, &action);
        Ok(Self {
            owner: owner.fingerprint.clone(),
            key: owner.user_key.clone(),
            file_id: file_id.to_string(),
            action,
            signature: sign(owner, &message)?,
        })
    }

    pub fn owner(&self, keys: &impl Keys) -> Option<String> {
        let message = Self::message(&self.owner, &self.key, &self.file_id, &self.action);
        let key = signer(&self.key, &message, &self.signature)?;
        (keys.owner_of(&key) == Some(self.owner.as_str())).then(|| self.owner.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub file_id: String,
    pub chunks: Vec<String>,
    pub chunk_bytes: u64,
    pub network: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub uploaded_at: u64,
    pub owner: String,
    pub wraps: BTreeMap<String, WrappedKey>,
    pub grants: BTreeMap<String, Grant>,
    pub revoked: BTreeSet<String>,
}

impl Manifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        owner: &Identity,
        family: &Family,
        network: &str,
        file_name: &str,
        size_bytes: u64,
        file_id: &str,
        chunks: Vec<String>,
        chunk_bytes: u64,
        key: &[u8; 32],
    ) -> BpResult<Self> {
        Ok(Self {
            file_id: file_id.to_string(),
            chunks,
            chunk_bytes,
            network: network.to_string(),
            file_name: file_name.to_string(),
            size_bytes,
            uploaded_at: chrono::Utc::now().timestamp() as u64,
            owner: owner.fingerprint.clone(),
            wraps: BTreeMap::from([(owner.fingerprint.clone(), family.wrap(key)?)]),
            grants: BTreeMap::from([(
                owner.fingerprint.clone(),
                Grant::sign(owner, file_id, &owner.fingerprint)?,
            )]),
            revoked: BTreeSet::new(),
        })
    }

    pub fn file_key(&self, identity: &Identity, keyring: &[[u8; 32]]) -> BpResult<[u8; 32]> {
        let wrapped = self.wraps.get(&identity.fingerprint).ok_or_else(|| {
            BpError::Identity(format!("'{}' is not shared with you", self.file_name))
        })?;
        open_with(wrapped, keyring)
    }

    pub fn grant_for(&self, user: &str) -> BpResult<&Grant> {
        self.grants.get(user).ok_or_else(|| {
            BpError::Identity(format!("no fetch permission for '{}'", self.file_name))
        })
    }

    fn ensure_owner(&self, identity: &Identity) -> BpResult<()> {
        if self.owner != identity.fingerprint {
            return Err(BpError::Identity(format!(
                "only the owner can change who reads '{}'",
                self.file_name
            )));
        }
        Ok(())
    }

    pub fn share_with(
        &mut self,
        identity: &Identity,
        keyring: &[[u8; 32]],
        member: &str,
        family: &Family,
    ) -> BpResult<()> {
        self.ensure_owner(identity)?;
        let key = self.file_key(identity, keyring)?;
        self.wraps.insert(member.to_string(), family.wrap(&key)?);
        self.grants.insert(
            member.to_string(),
            Grant::sign(identity, &self.file_id, member)?,
        );
        self.revoked.remove(member);
        Ok(())
    }

    pub fn revoke(&mut self, identity: &Identity, member: &str) -> BpResult<()> {
        self.ensure_owner(identity)?;
        if member == self.owner || !self.wraps.contains_key(member) {
            return Err(BpError::InvalidInput(format!(
                "'{}' is not shared with {member}",
                self.file_name
            )));
        }
        self.wraps.remove(member);
        self.grants.remove(member);
        self.revoked.insert(member.to_string());
        Ok(())
    }

    fn merge(&mut self, other: &Manifest) -> bool {
        let before = (self.wraps.len(), self.grants.len(), self.revoked.len());
        self.revoked.extend(other.revoked.iter().cloned());
        for (user, wrapped) in &other.wraps {
            if !self.revoked.contains(user) {
                self.wraps
                    .entry(user.clone())
                    .or_insert_with(|| wrapped.clone());
            }
        }
        for (user, grant) in &other.grants {
            if !self.revoked.contains(user) {
                self.grants
                    .entry(user.clone())
                    .or_insert_with(|| grant.clone());
            }
        }
        let revoked = &self.revoked;
        self.wraps.retain(|u, _| !revoked.contains(u));
        self.grants.retain(|u, _| !revoked.contains(u));
        (self.wraps.len(), self.grants.len(), self.revoked.len()) != before
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub version: u64,
    pub files: BTreeMap<String, Manifest>,
    pub deleted: BTreeSet<String>,
}

impl Index {
    pub fn add(&mut self, manifest: Manifest) -> bool {
        if self.deleted.contains(&manifest.file_id) {
            return false;
        }
        match self.files.get_mut(&manifest.file_id) {
            Some(existing) => existing.merge(&manifest),
            None => {
                self.files.insert(manifest.file_id.clone(), manifest);
                true
            }
        }
    }

    pub fn remove(&mut self, file_id: &str) -> Option<Manifest> {
        self.deleted.insert(file_id.to_string());
        self.files.remove(file_id)
    }

    pub fn merge(&mut self, other: &Index) -> bool {
        let mut changed = false;
        for file_id in &other.deleted {
            changed |= self.files.remove(file_id).is_some();
            changed |= self.deleted.insert(file_id.clone());
        }
        for manifest in other.files.values() {
            changed |= self.add(manifest.clone());
        }
        self.version = self.version.max(other.version);
        changed
    }

    pub fn seal(&self, keyring: &[[u8; 32]]) -> BpResult<Vec<u8>> {
        let current = keyring
            .first()
            .ok_or_else(|| BpError::Identity("no family key to seal the index".into()))?;
        ChunkCipher::from_raw_key(index_key(current)).encrypt(&serde_json::to_vec(self)?)
    }

    pub fn open(keyring: &[[u8; 32]], blob: &[u8]) -> BpResult<Self> {
        let plain = keyring
            .iter()
            .find_map(|secret| {
                ChunkCipher::from_raw_key(index_key(secret))
                    .decrypt(blob)
                    .ok()
            })
            .ok_or_else(|| BpError::Identity("none of your family keys opens this index".into()))?;
        Ok(serde_json::from_slice(&plain)?)
    }
}

fn index_key(family_secret: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key("billpouch/index-key/v2", family_secret)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    pub owner: String,
    pub key: String,
    pub network: String,
    pub version: u64,
    pub file_id: String,
    pub chunks: Vec<String>,
    pub signature: String,
}

impl Head {
    fn message(
        owner: &str,
        key: &str,
        network: &str,
        version: u64,
        file_id: &str,
        chunks: &[String],
    ) -> Vec<u8> {
        signed(
            b"billpouch/index-head/v3",
            &(owner, key, network, version, file_id, chunks),
        )
    }

    pub fn sign(
        identity: &Identity,
        network: &str,
        version: u64,
        file_id: &str,
        chunks: Vec<String>,
    ) -> BpResult<Self> {
        let message = Self::message(
            &identity.fingerprint,
            &identity.user_key,
            network,
            version,
            file_id,
            &chunks,
        );
        Ok(Self {
            owner: identity.fingerprint.clone(),
            key: identity.user_key.clone(),
            network: network.to_string(),
            version,
            file_id: file_id.to_string(),
            chunks,
            signature: sign(identity, &message)?,
        })
    }

    pub fn owner(&self, keys: &impl Keys) -> Option<String> {
        let message = Self::message(
            &self.owner,
            &self.key,
            &self.network,
            self.version,
            &self.file_id,
            &self.chunks,
        );
        let key = signer(&self.key, &message, &self.signature)?;
        (keys.owner_of(&key) == Some(self.owner.as_str())).then(|| self.owner.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingShare {
    pub network: String,
    pub to: String,
    pub manifest: Manifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingOrder {
    pub network: String,
    pub order: Order,
    pub sends: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FileStore {
    pub indexes: BTreeMap<String, Index>,
    pub heads: BTreeMap<String, BTreeMap<String, Head>>,
    pub outbox: Vec<PendingShare>,
    pub orders: Vec<PendingOrder>,
    #[serde(skip)]
    persistent: bool,
}

impl FileStore {
    pub fn load() -> Self {
        let mut store: Self = config::files_path()
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        store.persistent = true;
        store
    }

    pub fn save(&self) {
        if !self.persistent {
            return;
        }
        let result = config::files_path().and_then(|p| {
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(p, serde_json::to_vec(self)?)?;
            Ok(())
        });
        if let Err(e) = result {
            tracing::warn!("cannot save the file index: {e}");
        }
    }

    pub fn index(&self, network: &str) -> Option<&Index> {
        self.indexes.get(network)
    }

    pub fn head(&self, network: &str, owner: &str) -> Option<&Head> {
        self.heads.get(network)?.get(owner)
    }

    pub fn remember_head(&mut self, head: Head, keys: &impl Keys) -> Option<String> {
        let owner = head.owner(keys)?;
        let heads = self.heads.entry(head.network.clone()).or_default();
        if heads.get(&owner).is_some_and(|h| h.version >= head.version) {
            return None;
        }
        heads.insert(owner.clone(), head);
        Some(owner)
    }

    pub fn forget_network(&mut self, network: &str) {
        self.indexes.remove(network);
        self.heads.remove(network);
        self.outbox.retain(|s| s.network != network);
        self.orders.retain(|o| o.network != network);
        self.save();
    }

    pub fn queue_order(&mut self, network: &str, order: Order) {
        self.orders.push(PendingOrder {
            network: network.to_string(),
            order,
            sends: 0,
        });
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_are_compressed_only_when_it_pays() {
        let text = b"ciao come va ".repeat(10_000);
        let (packed, compressed) = pack(&text, true);
        assert!(compressed && packed.len() < text.len() / 10);
        assert_eq!(unpack(&packed).unwrap(), text);

        let noise: Vec<u8> = (0..100_000).map(|_| rand::random::<u8>()).collect();
        let (packed, compressed) = pack(&noise, true);
        assert!(!compressed);
        assert_eq!(packed.len(), noise.len() + 1);
        assert_eq!(unpack(&packed).unwrap(), noise);

        let (packed, compressed) = pack(&text, false);
        assert!(!compressed);
        assert_eq!(unpack(&packed).unwrap(), text);
        assert!(unpack(&[9, 1, 2]).is_err());
    }

    #[test]
    fn chunk_ids_depend_on_the_file_and_the_position() {
        assert_eq!(chunk_id("f", 0), chunk_id("f", 0));
        assert_ne!(chunk_id("f", 0), chunk_id("f", 1));
        assert_ne!(chunk_id("f", 0), chunk_id("g", 0));
        assert_eq!(chunk_ids("f", 3).count(), 3);
        assert_eq!(chunk_id("f", 0).len(), 16);
    }

    struct Book(BTreeMap<String, String>);

    impl Keys for Book {
        fn owner_of(&self, key: &str) -> Option<&str> {
            self.0.get(key).map(String::as_str)
        }

        fn ever_owned(&self, user: &str, key: &str) -> bool {
            self.owner_of(key) == Some(user)
        }
    }

    fn book(people: &[&Identity]) -> Book {
        Book(
            people
                .iter()
                .map(|p| (p.key_fingerprint.clone(), p.fingerprint.clone()))
                .collect(),
        )
    }

    fn family(person: &Identity) -> (Family, Vec<[u8; 32]>) {
        let keyring = vec![new_key()];
        let family = Family::seal(&keyring, [(&person.key_fingerprint, &person.user_key)]).unwrap();
        (family, keyring)
    }

    fn manifest(owner: &Identity, file_id: &str) -> (Manifest, [u8; 32], Vec<[u8; 32]>) {
        let key = new_key();
        let (family, keyring) = family(owner);
        let m = Manifest::new(
            owner,
            &family,
            "amici",
            "x",
            1,
            file_id,
            vec!["c1".into()],
            CHUNK_BYTES as u64,
            &key,
        )
        .unwrap();
        (m, key, keyring)
    }

    #[test]
    fn only_the_owner_and_the_people_it_is_shared_with_get_the_key() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let lucia = Identity::in_memory(None).unwrap();
        let (mut m, key, carlo_ring) = manifest(&carlo, "f1");
        let (marco_family, marco_ring) = family(&marco);
        let (lucia_family, lucia_ring) = family(&lucia);
        assert_eq!(m.file_key(&carlo, &carlo_ring).unwrap(), key);
        assert!(m.file_key(&marco, &marco_ring).is_err());

        m.share_with(&carlo, &carlo_ring, &marco.fingerprint, &marco_family)
            .unwrap();
        assert_eq!(m.file_key(&marco, &marco_ring).unwrap(), key);
        assert!(m.file_key(&lucia, &lucia_ring).is_err());
        assert!(m
            .share_with(&lucia, &lucia_ring, &lucia.fingerprint, &lucia_family)
            .is_err());
        assert!(m
            .share_with(&marco, &marco_ring, &lucia.fingerprint, &lucia_family)
            .is_err());
    }

    #[test]
    fn grants_name_one_file_one_owner_and_one_grantee() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let keys = book(&[&carlo, &marco]);
        let (mut m, _, ring) = manifest(&carlo, "f1");
        let own = m.grant_for(&carlo.fingerprint).unwrap().clone();
        assert!(own.is_owner_claim(&keys));
        assert!(!own.is_owner_claim(&book(&[&marco])));
        assert!(own.allows("f1", &carlo.fingerprint, &carlo.fingerprint, &keys));
        assert!(!own.allows("f1", &carlo.fingerprint, &marco.fingerprint, &keys));
        assert!(!own.allows("f2", &carlo.fingerprint, &carlo.fingerprint, &keys));

        m.share_with(&carlo, &ring, &marco.fingerprint, &family(&marco).0)
            .unwrap();
        let theirs = m.grant_for(&marco.fingerprint).unwrap();
        assert!(theirs.allows("f1", &carlo.fingerprint, &marco.fingerprint, &keys));
        assert!(!theirs.is_owner_claim(&keys));

        let mut forged = theirs.clone();
        forged.grantee = Identity::in_memory(None).unwrap().fingerprint;
        assert!(!forged.allows("f1", &carlo.fingerprint, &forged.grantee, &keys));
    }

    #[test]
    fn a_revoked_share_does_not_come_back_through_a_merge() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let (mut m, _, ring) = manifest(&carlo, "f1");
        m.share_with(&carlo, &ring, &marco.fingerprint, &family(&marco).0)
            .unwrap();
        let old = m.clone();
        assert!(m.revoke(&marco, &marco.fingerprint).is_err());
        m.revoke(&carlo, &marco.fingerprint).unwrap();
        assert!(m.revoke(&carlo, &carlo.fingerprint).is_err());

        let mut index = Index::default();
        index.add(m);
        index.add(old);
        let merged = &index.files["f1"];
        assert!(!merged.wraps.contains_key(&marco.fingerprint));
        assert!(merged.grant_for(&marco.fingerprint).is_err());
    }

    #[test]
    fn deleted_files_do_not_come_back_through_a_merge() {
        let carlo = Identity::in_memory(None).unwrap();
        let (m, _, _) = manifest(&carlo, "f1");
        let mut stale = Index::default();
        stale.add(m.clone());
        let mut index = Index::default();
        index.add(m.clone());
        assert!(index.remove("f1").is_some());
        assert!(!index.add(m));
        assert!(!index.merge(&stale));
        assert!(index.files.is_empty());
        assert!(stale.merge(&index));
        assert!(stale.files.is_empty());
    }

    #[test]
    fn every_key_of_the_family_opens_the_same_index_even_after_a_rotation() {
        let carlo = Identity::in_memory(None).unwrap();
        let mut index = Index::default();
        let (m, _, ring) = manifest(&carlo, "f1");
        index.add(m);
        let blob = index.seal(&ring).unwrap();
        assert_eq!(Index::open(&ring, &blob).unwrap(), index);
        let rotated = vec![new_key(), ring[0]];
        assert_eq!(Index::open(&rotated, &blob).unwrap(), index);
        assert!(Index::open(&[new_key()], &blob).is_err());
    }

    #[test]
    fn orders_and_heads_count_only_from_a_live_key_of_their_owner() {
        let carlo = Identity::in_memory(None).unwrap();
        let mallory = Identity::in_memory(None).unwrap();
        let keys = book(&[&carlo, &mallory]);
        let order = Order::sign(&carlo, "f1", Action::Delete).unwrap();
        assert_eq!(order.owner(&keys), Some(carlo.fingerprint.clone()));
        assert_eq!(order.owner(&book(&[&mallory])), None);
        let mut forged = order.clone();
        forged.file_id = "f2".into();
        assert_eq!(forged.owner(&keys), None);
        let mut impostor = Order::sign(&mallory, "f1", Action::Delete).unwrap();
        impostor.owner = carlo.fingerprint.clone();
        assert_eq!(impostor.owner(&keys), None);

        let mut store = FileStore::default();
        let h1 = Head::sign(&carlo, "amici", 1, "i1", vec!["c1".into()]).unwrap();
        let h2 = Head::sign(&carlo, "amici", 2, "i2", vec!["c2".into()]).unwrap();
        assert_eq!(
            store.remember_head(h2.clone(), &keys),
            Some(carlo.fingerprint.clone())
        );
        assert_eq!(store.remember_head(h1, &keys), None);
        let mut forged = h2.clone();
        forged.version = 9;
        assert_eq!(store.remember_head(forged, &keys), None);
        assert_eq!(store.head("amici", &carlo.fingerprint), Some(&h2));
    }
}
