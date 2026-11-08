use crate::{
    config,
    error::{BpError, BpResult},
    identity::{parse_user_key, Identity},
    membership::{new_key, unwrap_for, wrap_for, WrappedKey},
    storage::ChunkCipher,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub chunk_id: String,
    pub network: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub uploaded_at: u64,
    pub owner: String,
    pub wraps: BTreeMap<String, WrappedKey>,
}

pub struct Sealed {
    pub key: [u8; 32],
    pub ciphertext: Vec<u8>,
}

pub fn seal_file(data: &[u8]) -> BpResult<Sealed> {
    let key = new_key();
    let ciphertext = ChunkCipher::from_raw_key(key).encrypt(data)?;
    Ok(Sealed { key, ciphertext })
}

impl Manifest {
    pub fn new(
        owner: &Identity,
        network: &str,
        file_name: &str,
        size_bytes: u64,
        chunk_id: &str,
        key: &[u8; 32],
    ) -> BpResult<Self> {
        Ok(Self {
            chunk_id: chunk_id.to_string(),
            network: network.to_string(),
            file_name: file_name.to_string(),
            size_bytes,
            uploaded_at: chrono::Utc::now().timestamp() as u64,
            owner: owner.fingerprint.clone(),
            wraps: BTreeMap::from([(owner.fingerprint.clone(), wrap_for(key, &owner.user_key)?)]),
        })
    }

    pub fn file_key(&self, identity: &Identity) -> BpResult<[u8; 32]> {
        let wrapped = self.wraps.get(&identity.fingerprint).ok_or_else(|| {
            BpError::Identity(format!("'{}' is not shared with you", self.file_name))
        })?;
        unwrap_for(wrapped, identity)
    }

    pub fn open(&self, identity: &Identity, ciphertext: &[u8]) -> BpResult<Vec<u8>> {
        ChunkCipher::from_raw_key(self.file_key(identity)?).decrypt(ciphertext)
    }

    pub fn share_with(&mut self, identity: &Identity, member_key: &str) -> BpResult<String> {
        let key = self.file_key(identity)?;
        let (_, fingerprint) = parse_user_key(member_key)?;
        self.wraps
            .insert(fingerprint.clone(), wrap_for(&key, member_key)?);
        Ok(fingerprint)
    }

    fn merge(&mut self, other: &Manifest) -> bool {
        let before = self.wraps.len();
        for (user, wrapped) in &other.wraps {
            self.wraps
                .entry(user.clone())
                .or_insert_with(|| wrapped.clone());
        }
        self.wraps.len() > before
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub version: u64,
    pub files: BTreeMap<String, Manifest>,
}

impl Index {
    pub fn add(&mut self, manifest: Manifest) -> bool {
        match self.files.get_mut(&manifest.chunk_id) {
            Some(existing) => existing.merge(&manifest),
            None => {
                self.files.insert(manifest.chunk_id.clone(), manifest);
                true
            }
        }
    }

    pub fn merge(&mut self, other: &Index) -> bool {
        let mut changed = false;
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

fn index_key(identity: &Identity) -> [u8; 32] {
    blake3::derive_key("billpouch/index-key/v1", &identity.secret_material())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    pub owner_key: String,
    pub network: String,
    pub version: u64,
    pub chunk_id: String,
    pub signature: String,
}

impl Head {
    fn message(owner_key: &str, network: &str, version: u64, chunk_id: &str) -> Vec<u8> {
        let mut msg = b"billpouch/index-head/v1".to_vec();
        msg.extend_from_slice(
            &serde_json::to_vec(&(owner_key, network, version, chunk_id)).unwrap_or_default(),
        );
        msg
    }

    pub fn sign(
        identity: &Identity,
        network: &str,
        version: u64,
        chunk_id: &str,
    ) -> BpResult<Self> {
        let signature = identity
            .user
            .sign(&Self::message(
                &identity.user_key,
                network,
                version,
                chunk_id,
            ))
            .map_err(|e| BpError::Identity(format!("cannot sign index head: {e}")))?;
        Ok(Self {
            owner_key: identity.user_key.clone(),
            network: network.to_string(),
            version,
            chunk_id: chunk_id.to_string(),
            signature: hex::encode(signature),
        })
    }

    pub fn owner(&self) -> Option<String> {
        let (public, fingerprint) = parse_user_key(&self.owner_key).ok()?;
        let signature = hex::decode(&self.signature).ok()?;
        let message = Self::message(&self.owner_key, &self.network, self.version, &self.chunk_id);
        public.verify(&message, &signature).then_some(fingerprint)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingShare {
    pub network: String,
    pub to: String,
    pub manifest: Manifest,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FileStore {
    pub indexes: BTreeMap<String, Index>,
    pub heads: BTreeMap<String, BTreeMap<String, Head>>,
    pub outbox: Vec<PendingShare>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_owner_and_the_people_it_is_shared_with_can_open_a_file() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let lucia = Identity::in_memory(None).unwrap();
        let sealed = seal_file(b"ciao").unwrap();
        let mut manifest =
            Manifest::new(&carlo, "amici", "ciao.txt", 4, "chunk", &sealed.key).unwrap();
        assert_eq!(manifest.open(&carlo, &sealed.ciphertext).unwrap(), b"ciao");
        assert!(manifest.open(&marco, &sealed.ciphertext).is_err());

        assert_eq!(
            manifest.share_with(&carlo, &marco.user_key).unwrap(),
            marco.fingerprint
        );
        assert_eq!(manifest.open(&marco, &sealed.ciphertext).unwrap(), b"ciao");
        assert!(manifest.open(&lucia, &sealed.ciphertext).is_err());
        assert!(manifest.share_with(&lucia, &lucia.user_key).is_err());
    }

    #[test]
    fn every_device_of_a_user_opens_the_same_index() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let sealed = seal_file(b"x").unwrap();
        let mut index = Index::default();
        index.add(Manifest::new(&carlo, "amici", "x", 1, "c1", &sealed.key).unwrap());
        let blob = index.seal(&carlo).unwrap();
        let laptop = carlo.clone();
        assert_eq!(Index::open(&laptop, &blob).unwrap(), index);
        assert!(Index::open(&marco, &blob).is_err());
    }

    #[test]
    fn indexes_merge_files_and_shares() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let sealed = seal_file(b"x").unwrap();
        let base = Manifest::new(&carlo, "amici", "x", 1, "c1", &sealed.key).unwrap();
        let mut shared = base.clone();
        shared.share_with(&carlo, &marco.user_key).unwrap();

        let mut a = Index::default();
        a.add(base);
        let mut b = Index {
            version: 7,
            ..Index::default()
        };
        b.add(shared);
        b.add(Manifest::new(&carlo, "amici", "y", 1, "c2", &sealed.key).unwrap());
        assert!(a.merge(&b));
        assert!(!a.merge(&b));
        assert_eq!(a.version, 7);
        assert_eq!(a.files.len(), 2);
        assert!(a.files["c1"].wraps.contains_key(&marco.fingerprint));
    }

    #[test]
    fn heads_are_signed_by_their_owner_and_only_newer_ones_are_kept() {
        let carlo = Identity::in_memory(None).unwrap();
        let mut store = FileStore::default();
        let h1 = Head::sign(&carlo, "amici", 1, "c1").unwrap();
        let h2 = Head::sign(&carlo, "amici", 2, "c2").unwrap();
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
