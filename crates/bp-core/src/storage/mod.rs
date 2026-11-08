pub mod encryption;
pub mod filler;
pub mod fragment;
pub mod merkle;
pub mod meta;
pub mod tier;

pub use encryption::ChunkCipher;
pub use fragment::{FragmentIndex, FragmentMeta};
pub use meta::PouchMeta;
pub use tier::StorageTier;

use crate::{
    coding::rlnc::EncodedFragment,
    config,
    error::{BpError, BpResult},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkOwner {
    pub owner_key: String,
    pub file_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PosTarget {
    Fragment {
        chunk_id: String,
        fragment_id: String,
    },
    Filler {
        blob_id: String,
    },
}

pub struct StorageManager {
    root: PathBuf,
    pub meta: PouchMeta,
    pub index: FragmentIndex,
    fillers: BTreeMap<String, u64>,
}

impl StorageManager {
    pub fn init(network_id: String, service_id: String, storage_bytes_bid: u64) -> BpResult<Self> {
        let root = Self::root_path(&network_id, &service_id)?;
        let fragments_dir = root.join("fragments");
        std::fs::create_dir_all(&fragments_dir).map_err(BpError::Io)?;

        let meta = PouchMeta::new(network_id, service_id, storage_bytes_bid);
        meta.save(&root.join("meta.json"))?;

        Ok(Self {
            root,
            meta,
            index: FragmentIndex::new(),
            fillers: BTreeMap::new(),
        })
    }

    pub fn load(network_id: &str, service_id: &str) -> BpResult<Self> {
        let root = Self::root_path(network_id, service_id)?;
        let meta_path = root.join("meta.json");
        if !meta_path.exists() {
            return Err(BpError::Storage(format!(
                "No storage found for service {service_id} on network {network_id}"
            )));
        }

        let meta = PouchMeta::load(&meta_path)?;
        let index = Self::build_index(&root)?;
        let fillers = std::fs::read_dir(root.join("fillers"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| Some((e.file_name().into_string().ok()?, e.metadata().ok()?.len())))
            .collect();

        Ok(Self {
            root,
            meta,
            index,
            fillers,
        })
    }

    pub fn owner_of(&self, chunk_id: &str) -> Option<ChunkOwner> {
        let bytes = std::fs::read(self.root.join("fragments").join(chunk_id).join("owner")).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn revoked(&self) -> BTreeMap<String, BTreeSet<String>> {
        std::fs::read(self.root.join("revoked.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn is_revoked(&self, file_id: &str, member: &str) -> bool {
        self.revoked()
            .get(file_id)
            .is_some_and(|members| members.contains(member))
    }

    fn chunks_of(&self, file_id: &str, owner_key: &str) -> Vec<String> {
        self.index
            .chunk_ids()
            .filter(|chunk| {
                self.owner_of(chunk)
                    .is_some_and(|o| o.file_id == file_id && o.owner_key == owner_key)
            })
            .map(str::to_string)
            .collect()
    }

    pub fn revoke(&mut self, file_id: &str, owner_key: &str, member: &str) -> BpResult<bool> {
        if self.chunks_of(file_id, owner_key).is_empty() {
            return Ok(false);
        }
        let mut revoked = self.revoked();
        revoked
            .entry(file_id.to_string())
            .or_default()
            .insert(member.to_string());
        std::fs::write(
            self.root.join("revoked.json"),
            serde_json::to_vec(&revoked)?,
        )?;
        Ok(true)
    }

    pub fn delete_file(&mut self, file_id: &str, owner_key: &str) -> BpResult<u64> {
        let mut freed = 0;
        for chunk in self.chunks_of(file_id, owner_key) {
            freed += self.index.remove_chunk(&chunk);
            let _ = std::fs::remove_dir_all(self.root.join("fragments").join(&chunk));
        }
        if freed > 0 {
            self.meta.storage_bytes_used = self.meta.storage_bytes_used.saturating_sub(freed);
            self.save_meta()?;
        }
        Ok(freed)
    }

    pub fn filler_bytes(&self) -> u64 {
        self.fillers.values().sum()
    }

    pub fn store_filler(&mut self, blob_id: &str, data: &[u8]) -> BpResult<()> {
        let size = data.len() as u64;
        if !self.meta.has_capacity(size) || self.fillers.contains_key(blob_id) {
            return Err(BpError::Storage("no room for filler".into()));
        }
        let dir = self.root.join("fillers");
        std::fs::create_dir_all(&dir).map_err(BpError::Io)?;
        std::fs::write(dir.join(blob_id), data).map_err(BpError::Io)?;
        self.fillers.insert(blob_id.to_string(), size);
        self.meta.storage_bytes_used += size;
        self.save_meta()
    }

    fn evict_fillers(&mut self, needed: u64) -> BpResult<Vec<String>> {
        let mut evicted = Vec::new();
        while !self.meta.has_capacity(needed) {
            let Some((blob_id, size)) = self.fillers.pop_first() else {
                break;
            };
            let _ = std::fs::remove_file(self.root.join("fillers").join(&blob_id));
            self.meta.storage_bytes_used = self.meta.storage_bytes_used.saturating_sub(size);
            evicted.push(blob_id);
        }
        if !evicted.is_empty() {
            self.save_meta()?;
        }
        Ok(evicted)
    }

    pub fn prove(&self, target: &PosTarget, index: usize) -> Option<merkle::Proof> {
        let data = match target {
            PosTarget::Fragment {
                chunk_id,
                fragment_id,
            } => self.load_fragment(chunk_id, fragment_id).ok()?.to_bytes(),
            PosTarget::Filler { blob_id } => {
                if !self.fillers.contains_key(blob_id) {
                    return None;
                }
                std::fs::read(self.root.join("fillers").join(blob_id)).ok()?
            }
        };
        merkle::prove(&data, index)
    }

    pub fn store_fragment(
        &mut self,
        fragment: &EncodedFragment,
        owner: &ChunkOwner,
    ) -> BpResult<Vec<String>> {
        if self
            .owner_of(&fragment.chunk_id)
            .is_some_and(|known| known != *owner)
        {
            return Err(BpError::Storage(format!(
                "chunk {} belongs to someone else",
                fragment.chunk_id
            )));
        }
        let bytes = fragment.to_bytes();
        let size = bytes.len() as u64;
        let evicted = self.evict_fillers(size)?;

        if !self.meta.has_capacity(size) {
            return Err(BpError::Storage(format!(
                "Quota exceeded: need {size} bytes, only {} available",
                self.meta.available_bytes()
            )));
        }

        let chunk_dir = self.root.join("fragments").join(&fragment.chunk_id);
        std::fs::create_dir_all(&chunk_dir).map_err(BpError::Io)?;
        std::fs::write(chunk_dir.join("owner"), serde_json::to_vec(owner)?).map_err(BpError::Io)?;

        let frag_path = chunk_dir.join(format!("{}.frag", fragment.id));
        std::fs::write(&frag_path, &bytes).map_err(BpError::Io)?;

        self.meta.storage_bytes_used += size;
        self.save_meta()?;

        self.index.insert(FragmentMeta {
            fragment_id: fragment.id.clone(),
            chunk_id: fragment.chunk_id.clone(),
            k: fragment.k,
            size_bytes: size,
        });

        Ok(evicted)
    }

    pub fn load_fragment(&self, chunk_id: &str, fragment_id: &str) -> BpResult<EncodedFragment> {
        let path = self
            .root
            .join("fragments")
            .join(chunk_id)
            .join(format!("{fragment_id}.frag"));

        if !path.exists() {
            return Err(BpError::Storage(format!(
                "Fragment not found: {chunk_id}/{fragment_id}"
            )));
        }

        let bytes = std::fs::read(&path).map_err(BpError::Io)?;
        EncodedFragment::from_bytes(fragment_id.to_string(), chunk_id.to_string(), &bytes)
    }

    pub fn has_capacity(&self, bytes: u64) -> bool {
        self.meta.has_capacity(bytes)
    }

    pub fn purge(&self) -> BpResult<()> {
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root).map_err(BpError::Io)?;
        }
        tracing::info!(root=?self.root, "Pouch storage purged");
        Ok(())
    }

    pub fn storage_summary(&self) -> (usize, usize, u64) {
        let chunks = self.index.chunk_ids().count();
        let frags = self.index.fragment_count();
        let bytes = self.index.total_bytes();
        (chunks, frags, bytes)
    }

    fn root_path(network_id: &str, service_id: &str) -> BpResult<PathBuf> {
        Ok(config::base_dir()?
            .join("storage")
            .join(network_id)
            .join(service_id))
    }

    fn save_meta(&self) -> BpResult<()> {
        self.meta.save(&self.root.join("meta.json"))
    }

    fn build_index(root: &Path) -> BpResult<FragmentIndex> {
        let fragments_dir = root.join("fragments");
        let mut index = FragmentIndex::new();

        if !fragments_dir.exists() {
            return Ok(index);
        }

        for chunk_entry in std::fs::read_dir(&fragments_dir).map_err(BpError::Io)? {
            let chunk_entry = chunk_entry.map_err(BpError::Io)?;
            let chunk_id = chunk_entry.file_name().to_string_lossy().to_string();

            for frag_entry in std::fs::read_dir(chunk_entry.path()).map_err(BpError::Io)? {
                let frag_entry = frag_entry.map_err(BpError::Io)?;
                let fname = frag_entry.file_name().to_string_lossy().to_string();
                if !fname.ends_with(".frag") {
                    continue;
                }
                let fragment_id = fname.trim_end_matches(".frag").to_string();
                let size_bytes = frag_entry.metadata().map(|m| m.len()).unwrap_or(0);

                let path = frag_entry.path();
                let header = std::fs::read(&path).map_err(BpError::Io)?;
                let k = if header.len() >= 8 {
                    u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize
                } else {
                    continue;
                };

                index.insert(FragmentMeta {
                    fragment_id,
                    chunk_id: chunk_id.clone(),
                    k,
                    size_bytes,
                });
            }
        }

        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding::rlnc;
    use std::sync::Mutex;

    static STORAGE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn owner(key: &str) -> ChunkOwner {
        ChunkOwner {
            owner_key: key.into(),
            file_id: "file".into(),
        }
    }

    fn with_temp_home_storage<F: FnOnce() + std::panic::UnwindSafe>(f: F) {
        let _guard = STORAGE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp =
            std::env::temp_dir().join(format!("bp_stor_test_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&tmp).unwrap();
        let old_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &tmp);

        let result = std::panic::catch_unwind(f);

        if let Some(h) = old_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
        std::fs::remove_dir_all(&tmp).ok();
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    const CHUNK: &[u8] = b"StorageManager test data - enough bytes to split into 4 symbols.";

    #[test]
    #[cfg(unix)]
    fn init_creates_directory_and_meta() {
        with_temp_home_storage(|| {
            let sm = StorageManager::init("amici".into(), "svc-1".into(), 1_000_000).unwrap();
            assert!(sm.root.join("meta.json").exists());
            assert!(sm.root.join("fragments").exists());
            assert_eq!(sm.meta.storage_bytes_bid, 1_000_000);
            assert_eq!(sm.meta.storage_bytes_used, 0);
        });
    }

    #[test]
    #[cfg(unix)]
    fn store_and_load_fragment() {
        with_temp_home_storage(|| {
            let mut sm = StorageManager::init("net".into(), "svc".into(), 1_000_000).unwrap();
            let frags = rlnc::encode(CHUNK, 4, 6).unwrap();

            sm.store_fragment(&frags[0], &owner("carlo")).unwrap();

            let loaded = sm.load_fragment(&frags[0].chunk_id, &frags[0].id).unwrap();
            assert_eq!(loaded.coding_vector, frags[0].coding_vector);
            assert_eq!(loaded.data, frags[0].data);
            assert_eq!(sm.meta.storage_bytes_used, frags[0].to_bytes().len() as u64);
        });
    }

    #[test]
    #[cfg(unix)]
    fn a_chunk_keeps_the_owner_of_its_first_fragment() {
        with_temp_home_storage(|| {
            let mut sm = StorageManager::init("net".into(), "svc".into(), 1_000_000).unwrap();
            let frags = rlnc::encode(CHUNK, 2, 3).unwrap();
            sm.store_fragment(&frags[0], &owner("carlo")).unwrap();
            assert!(sm.store_fragment(&frags[1], &owner("mallory")).is_err());
            sm.store_fragment(&frags[1], &owner("carlo")).unwrap();
            assert_eq!(sm.owner_of(&frags[0].chunk_id), Some(owner("carlo")));
            assert_eq!(sm.index.fragment_count(), 2);
        });
    }

    #[test]
    #[cfg(unix)]
    fn the_owner_can_revoke_and_delete_and_nobody_else() {
        with_temp_home_storage(|| {
            let mut sm = StorageManager::init("net".into(), "svc".into(), 1_000_000).unwrap();
            let frags = rlnc::encode(CHUNK, 2, 2).unwrap();
            sm.store_fragment(&frags[0], &owner("carlo")).unwrap();
            sm.store_fragment(&frags[1], &owner("carlo")).unwrap();
            assert!(!sm.revoke("file", "mallory", "marco").unwrap());
            assert!(!sm.is_revoked("file", "marco"));
            assert!(sm.revoke("file", "carlo", "marco").unwrap());
            assert!(sm.is_revoked("file", "marco"));

            assert_eq!(sm.delete_file("file", "mallory").unwrap(), 0);
            let used = sm.meta.storage_bytes_used;
            assert_eq!(sm.delete_file("file", "carlo").unwrap(), used);
            assert_eq!(sm.meta.storage_bytes_used, 0);
            assert_eq!(sm.index.fragment_count(), 0);
            assert!(sm.owner_of(&frags[0].chunk_id).is_none());
        });
    }

    #[test]
    #[cfg(unix)]
    fn fragments_push_fillers_out_and_both_can_be_proven() {
        with_temp_home_storage(|| {
            let frags = rlnc::encode(CHUNK, 1, 1).unwrap();
            let size = frags[0].to_bytes().len() as u64;
            let mut sm = StorageManager::init("net".into(), "svc".into(), size + 10).unwrap();
            let filler = vec![7u8; size as usize];
            sm.store_filler("blob", &filler).unwrap();
            assert!(sm.store_filler("more", &filler).is_err());
            let blob = PosTarget::Filler {
                blob_id: "blob".into(),
            };
            let proof = sm.prove(&blob, 0).unwrap();
            let blocks = merkle::block_count(filler.len());
            assert!(merkle::verify(&merkle::root(&filler), blocks, &proof));

            let evicted = sm.store_fragment(&frags[0], &owner("carlo")).unwrap();
            assert_eq!(evicted, vec!["blob".to_string()]);
            assert_eq!(sm.filler_bytes(), 0);
            assert!(sm.prove(&blob, 0).is_none());
            let fragment = PosTarget::Fragment {
                chunk_id: frags[0].chunk_id.clone(),
                fragment_id: frags[0].id.clone(),
            };
            let proof = sm.prove(&fragment, 0).unwrap();
            assert!(merkle::verify(
                &merkle::root(&frags[0].to_bytes()),
                1,
                &proof
            ));
        });
    }

    #[test]
    #[cfg(unix)]
    fn quota_exceeded_returns_error() {
        with_temp_home_storage(|| {
            let mut sm = StorageManager::init("net".into(), "svc".into(), 1).unwrap();
            let frags = rlnc::encode(CHUNK, 4, 4).unwrap();
            assert!(sm.store_fragment(&frags[0], &owner("carlo")).is_err());
        });
    }

    #[test]
    #[cfg(unix)]
    fn load_rebuilds_index_from_disk() {
        with_temp_home_storage(|| {
            let network_id = "net";
            let service_id = "svc";
            let frags = rlnc::encode(CHUNK, 4, 4).unwrap();

            {
                let mut sm =
                    StorageManager::init(network_id.into(), service_id.into(), 1_000_000).unwrap();
                sm.store_fragment(&frags[0], &owner("carlo")).unwrap();
                sm.store_fragment(&frags[1], &owner("carlo")).unwrap();
            }

            let sm2 = StorageManager::load(network_id, service_id).unwrap();
            assert_eq!(sm2.index.fragment_count(), 2);
            assert_eq!(sm2.meta.storage_bytes_used, sm2.index.total_bytes());
        });
    }
}
