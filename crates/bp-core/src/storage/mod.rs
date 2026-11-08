pub mod encryption;
pub mod fragment;
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
use std::path::{Path, PathBuf};

pub struct StorageManager {
    root: PathBuf,
    pub meta: PouchMeta,
    pub index: FragmentIndex,
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

        Ok(Self { root, meta, index })
    }

    pub fn store_fragment(&mut self, fragment: &EncodedFragment) -> BpResult<()> {
        let bytes = fragment.to_bytes();
        let size = bytes.len() as u64;

        if !self.meta.has_capacity(size) {
            return Err(BpError::Storage(format!(
                "Quota exceeded: need {size} bytes, only {} available",
                self.meta.available_bytes()
            )));
        }

        let chunk_dir = self.root.join("fragments").join(&fragment.chunk_id);
        std::fs::create_dir_all(&chunk_dir).map_err(BpError::Io)?;

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

        Ok(())
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

            sm.store_fragment(&frags[0]).unwrap();

            let loaded = sm.load_fragment(&frags[0].chunk_id, &frags[0].id).unwrap();
            assert_eq!(loaded.coding_vector, frags[0].coding_vector);
            assert_eq!(loaded.data, frags[0].data);
            assert_eq!(sm.meta.storage_bytes_used, frags[0].to_bytes().len() as u64);
        });
    }

    #[test]
    #[cfg(unix)]
    fn quota_exceeded_returns_error() {
        with_temp_home_storage(|| {
            let mut sm = StorageManager::init("net".into(), "svc".into(), 1).unwrap();
            let frags = rlnc::encode(CHUNK, 4, 4).unwrap();
            assert!(sm.store_fragment(&frags[0]).is_err());
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
                sm.store_fragment(&frags[0]).unwrap();
                sm.store_fragment(&frags[1]).unwrap();
            }

            let sm2 = StorageManager::load(network_id, service_id).unwrap();
            assert_eq!(sm2.index.fragment_count(), 2);
            assert_eq!(sm2.meta.storage_bytes_used, sm2.index.total_bytes());
        });
    }
}
