use crate::{
    config,
    error::{BpError, BpResult},
    identity::{parse_user_key, Identity},
    membership::{new_key, unwrap_for, wrap_for, WrappedKey},
    storage::ChunkCipher,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CHUNK_BYTES: usize = 4 * 1024 * 1024;
pub const ORDER_RESENDS: u32 = 10;

pub fn new_file_id() -> String {
    hex::encode(&new_key()[..8])
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

fn signer(owner_key: &str, message: &[u8], signature: &str) -> Option<String> {
    let (public, fingerprint) = parse_user_key(owner_key).ok()?;
    let signature = hex::decode(signature).ok()?;
    public.verify(message, &signature).then_some(fingerprint)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub file_id: String,
    pub owner_key: String,
    pub grantee: String,
    pub signature: String,
}

impl Grant {
    fn message(file_id: &str, owner_key: &str, grantee: &str) -> Vec<u8> {
        signed(b"billpouch/grant/v2", &(file_id, owner_key, grantee))
    }

    pub fn sign(owner: &Identity, file_id: &str, grantee: &str) -> BpResult<Self> {
        Ok(Self {
            file_id: file_id.to_string(),
            owner_key: owner.user_key.clone(),
            grantee: grantee.to_string(),
            signature: sign(owner, &Self::message(file_id, &owner.user_key, grantee))?,
        })
    }

    pub fn owner(&self) -> Option<String> {
        let message = Self::message(&self.file_id, &self.owner_key, &self.grantee);
        signer(&self.owner_key, &message, &self.signature)
    }

    pub fn allows(&self, file_id: &str, owner_key: &str, requester: &str) -> bool {
        self.file_id == file_id
            && self.owner_key == owner_key
            && self.grantee == requester
            && self.owner().is_some()
    }

    pub fn is_owner_claim(&self) -> bool {
        self.owner().as_deref() == Some(self.grantee.as_str())
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
    pub owner_key: String,
    pub file_id: String,
    pub action: Action,
    pub signature: String,
}

impl Order {
    fn message(owner_key: &str, file_id: &str, action: &Action) -> Vec<u8> {
        signed(b"billpouch/order/v1", &(owner_key, file_id, action))
    }

    pub fn sign(owner: &Identity, file_id: &str, action: Action) -> BpResult<Self> {
        let signature = sign(owner, &Self::message(&owner.user_key, file_id, &action))?;
        Ok(Self {
            owner_key: owner.user_key.clone(),
            file_id: file_id.to_string(),
            action,
            signature,
        })
    }

    pub fn owner(&self) -> Option<String> {
        let message = Self::message(&self.owner_key, &self.file_id, &self.action);
        signer(&self.owner_key, &message, &self.signature)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub file_id: String,
    pub chunks: Vec<String>,
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
        network: &str,
        file_name: &str,
        size_bytes: u64,
        file_id: &str,
        chunks: Vec<String>,
        key: &[u8; 32],
    ) -> BpResult<Self> {
        Ok(Self {
            file_id: file_id.to_string(),
            chunks,
            network: network.to_string(),
            file_name: file_name.to_string(),
            size_bytes,
            uploaded_at: chrono::Utc::now().timestamp() as u64,
            owner: owner.fingerprint.clone(),
            wraps: BTreeMap::from([(owner.fingerprint.clone(), wrap_for(key, &owner.user_key)?)]),
            grants: BTreeMap::from([(
                owner.fingerprint.clone(),
                Grant::sign(owner, file_id, &owner.fingerprint)?,
            )]),
            revoked: BTreeSet::new(),
        })
    }

    pub fn file_key(&self, identity: &Identity) -> BpResult<[u8; 32]> {
        let wrapped = self.wraps.get(&identity.fingerprint).ok_or_else(|| {
            BpError::Identity(format!("'{}' is not shared with you", self.file_name))
        })?;
        unwrap_for(wrapped, identity)
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

    pub fn share_with(&mut self, identity: &Identity, member_key: &str) -> BpResult<String> {
        self.ensure_owner(identity)?;
        let key = self.file_key(identity)?;
        let (_, fingerprint) = parse_user_key(member_key)?;
        self.wraps
            .insert(fingerprint.clone(), wrap_for(&key, member_key)?);
        self.grants.insert(
            fingerprint.clone(),
            Grant::sign(identity, &self.file_id, &fingerprint)?,
        );
        self.revoked.remove(&fingerprint);
        Ok(fingerprint)
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

    pub fn seal(&self, identity: &Identity) -> BpResult<Vec<u8>> {
        ChunkCipher::from_raw_key(index_key(identity)).encrypt(&serde_json::to_vec(self)?)
    }

    pub fn open(identity: &Identity, blob: &[u8]) -> BpResult<Self> {
        let plain = ChunkCipher::from_raw_key(index_key(identity)).decrypt(blob)?;
        Ok(serde_json::from_slice(&plain)?)
    }
}

pub fn index_key(identity: &Identity) -> [u8; 32] {
    blake3::derive_key("billpouch/index-key/v1", &identity.secret_material())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    pub owner_key: String,
    pub network: String,
    pub version: u64,
    pub file_id: String,
    pub chunks: Vec<String>,
    pub signature: String,
}

impl Head {
    fn message(
        owner_key: &str,
        network: &str,
        version: u64,
        file_id: &str,
        chunks: &[String],
    ) -> Vec<u8> {
        signed(
            b"billpouch/index-head/v2",
            &(owner_key, network, version, file_id, chunks),
        )
    }

    pub fn sign(
        identity: &Identity,
        network: &str,
        version: u64,
        file_id: &str,
        chunks: Vec<String>,
    ) -> BpResult<Self> {
        let message = Self::message(&identity.user_key, network, version, file_id, &chunks);
        Ok(Self {
            owner_key: identity.user_key.clone(),
            network: network.to_string(),
            version,
            file_id: file_id.to_string(),
            chunks,
            signature: sign(identity, &message)?,
        })
    }

    pub fn owner(&self) -> Option<String> {
        let message = Self::message(
            &self.owner_key,
            &self.network,
            self.version,
            &self.file_id,
            &self.chunks,
        );
        signer(&self.owner_key, &message, &self.signature)
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

    pub fn remember_head(&mut self, head: Head) -> Option<String> {
        let owner = head.owner()?;
        let heads = self.heads.entry(head.network.clone()).or_default();
        if heads.get(&owner).is_some_and(|h| h.version >= head.version) {
            return None;
        }
        heads.insert(owner.clone(), head);
        Some(owner)
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

    fn manifest(owner: &Identity, file_id: &str) -> (Manifest, [u8; 32]) {
        let key = new_key();
        let m = Manifest::new(owner, "amici", "x", 1, file_id, vec!["c1".into()], &key).unwrap();
        (m, key)
    }

    #[test]
    fn only_the_owner_and_the_people_it_is_shared_with_get_the_key() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let lucia = Identity::in_memory(None).unwrap();
        let (mut m, key) = manifest(&carlo, "f1");
        assert_eq!(m.file_key(&carlo).unwrap(), key);
        assert!(m.file_key(&marco).is_err());

        assert_eq!(
            m.share_with(&carlo, &marco.user_key).unwrap(),
            marco.fingerprint
        );
        assert_eq!(m.file_key(&marco).unwrap(), key);
        assert!(m.file_key(&lucia).is_err());
        assert!(m.share_with(&lucia, &lucia.user_key).is_err());
        assert!(m.share_with(&marco, &lucia.user_key).is_err());
    }

    #[test]
    fn grants_name_one_file_one_owner_and_one_grantee() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let (mut m, _) = manifest(&carlo, "f1");
        let own = m.grant_for(&carlo.fingerprint).unwrap().clone();
        assert!(own.is_owner_claim());
        assert!(own.allows("f1", &carlo.user_key, &carlo.fingerprint));
        assert!(!own.allows("f1", &carlo.user_key, &marco.fingerprint));
        assert!(!own.allows("f2", &carlo.user_key, &carlo.fingerprint));

        m.share_with(&carlo, &marco.user_key).unwrap();
        let theirs = m.grant_for(&marco.fingerprint).unwrap();
        assert!(theirs.allows("f1", &carlo.user_key, &marco.fingerprint));
        assert!(!theirs.is_owner_claim());

        let mut forged = theirs.clone();
        forged.grantee = Identity::in_memory(None).unwrap().fingerprint;
        assert!(!forged.allows("f1", &carlo.user_key, &forged.grantee));
    }

    #[test]
    fn a_revoked_share_does_not_come_back_through_a_merge() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let (mut m, _) = manifest(&carlo, "f1");
        m.share_with(&carlo, &marco.user_key).unwrap();
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
        let (m, _) = manifest(&carlo, "f1");
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
    fn every_device_of_a_user_opens_the_same_index() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let mut index = Index::default();
        index.add(manifest(&carlo, "f1").0);
        let blob = index.seal(&carlo).unwrap();
        assert_eq!(Index::open(&carlo.clone(), &blob).unwrap(), index);
        assert!(Index::open(&marco, &blob).is_err());
    }

    #[test]
    fn orders_and_heads_are_signed_by_their_owner() {
        let carlo = Identity::in_memory(None).unwrap();
        let order = Order::sign(&carlo, "f1", Action::Delete).unwrap();
        assert_eq!(order.owner(), Some(carlo.fingerprint.clone()));
        let mut forged = order.clone();
        forged.file_id = "f2".into();
        assert_eq!(forged.owner(), None);

        let mut store = FileStore::default();
        let h1 = Head::sign(&carlo, "amici", 1, "i1", vec!["c1".into()]).unwrap();
        let h2 = Head::sign(&carlo, "amici", 2, "i2", vec!["c2".into()]).unwrap();
        assert_eq!(
            store.remember_head(h2.clone()),
            Some(carlo.fingerprint.clone())
        );
        assert_eq!(store.remember_head(h1), None);
        let mut forged = h2.clone();
        forged.version = 9;
        assert_eq!(store.remember_head(forged), None);
        assert_eq!(store.head("amici", &carlo.fingerprint), Some(&h2));
    }
}
