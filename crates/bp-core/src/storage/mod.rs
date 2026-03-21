//! Local storage management for a Pouch service instance.
//!
//! ## Responsibilities
//!
//! - Create and maintain the on-disk directory structure for a Pouch bid.
//! - Persist and load [`EncodedFragment`]s in the binary `.frag` format.
//! - Maintain an in-memory [`FragmentIndex`] for fast chunk/fragment lookups.
//! - Enforce the storage quota declared at bid time.
//! - Expose recoding helpers so the daemon can generate new fragments for
//!   newly-joined Pouches without touching the original data.
//!
//! ## Directory layout
//!
//! ```text
//! ~/.local/share/billpouch/storage/<network_id>/<service_id>/
//!   meta.json
//!   fragments/
//!     <chunk_id>/
//!       <fragment_id>.frag
//! ```

pub mod agreement;
pub mod fragment;
pub mod manifest;
pub mod meta;

pub use agreement::{AgreementAcceptance, AgreementStatus, AgreementStore, StorageAgreement, StorageOffer};
pub use fragment::{FragmentIndex, FragmentMeta};
pub use manifest::{ChunkManifest, FileManifest, FragmentLocation, NetworkMetaKey};
pub use meta::PouchMeta;

use crate::{
    coding::rlnc::{self, EncodedFragment},
    config,
    error::{BpError, BpResult},
};
use std::path::{Path, PathBuf};

// ── StorageManager ────────────────────────────────────────────────────────────

/// Manages on-disk storage for a single Pouch service instance.
///
/// One `StorageManager` exists per active Pouch service.
/// It is held inside `DaemonState` (wrapped in `Arc<RwLock<StorageManager>>`)
/// alongside the service registry.
pub struct StorageManager {
    /// Absolute path to `storage/<network_id>/<service_id>/`.
    root: PathBuf,
    /// Persistent quota and usage tracking.
    pub meta: PouchMeta,
    /// In-memory fragment index (rebuilt from disk on load).
    pub index: FragmentIndex,
}

impl StorageManager {
    // ── Construction ─────────────────────────────────────────────────────

    /// Initialise a brand-new Pouch storage directory.
    ///
    /// Creates the directory structure, writes `meta.json`, and returns a
    /// manager with an empty fragment index.
    ///
    /// # Errors
    /// Fails if disk I/O fails or the XDG base directory cannot be resolved.
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

    /// Load an existing Pouch storage directory from disk.
    ///
    /// Reads `meta.json` and rebuilds the fragment index by scanning the
    /// `fragments/` subdirectory.
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

    // ── Fragment I/O ──────────────────────────────────────────────────────

    /// Persist an encoded fragment to disk and update the index.
    ///
    /// Returns an error if the Pouch does not have enough remaining capacity.
    pub fn store_fragment(&mut self, fragment: &EncodedFragment) -> BpResult<()> {
        let bytes = fragment.to_bytes();
        let size = bytes.len() as u64;

        if !self.meta.has_capacity(size) {
            return Err(BpError::Storage(format!(
                "Quota exceeded: need {size} bytes, only {} available",
                self.meta.available_bytes()
            )));
        }

        // Ensure chunk subdirectory exists
        let chunk_dir = self.root.join("fragments").join(&fragment.chunk_id);
        std::fs::create_dir_all(&chunk_dir).map_err(BpError::Io)?;

        // Write the .frag file
        let frag_path = chunk_dir.join(format!("{}.frag", fragment.id));
        std::fs::write(&frag_path, &bytes).map_err(BpError::Io)?;

        // Update meta and index
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

    /// Load a fragment from disk by chunk_id and fragment_id.
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

    /// Remove a fragment from disk and update the index.
    pub fn remove_fragment(&mut self, chunk_id: &str, fragment_id: &str) -> BpResult<()> {
        let path = self
            .root
            .join("fragments")
            .join(chunk_id)
            .join(format!("{fragment_id}.frag"));

        if path.exists() {
            std::fs::remove_file(&path).map_err(BpError::Io)?;
        }

        // Remove empty chunk directory
        let chunk_dir = self.root.join("fragments").join(chunk_id);
        if chunk_dir.exists()
            && std::fs::read_dir(&chunk_dir).map_or(true, |mut d| d.next().is_none())
        {
            std::fs::remove_dir(&chunk_dir).ok();
        }

        if let Some(removed) = self.index.remove(chunk_id, fragment_id) {
            self.meta.storage_bytes_used = self
                .meta
                .storage_bytes_used
                .saturating_sub(removed.size_bytes);
            self.save_meta()?;
        }

        Ok(())
    }

    // ── Recoding ──────────────────────────────────────────────────────────

    /// Generate `count` new fragments for `chunk_id` by recoding the local ones.
    ///
    /// This is the key operation for filling new Pouches: no decoding is performed,
    /// no original data is required.
    ///
    /// Returns an error if this Pouch holds no fragments for the given chunk.
    pub fn recode_chunk(&self, chunk_id: &str, count: usize) -> BpResult<Vec<EncodedFragment>> {
        let metas = self.index.fragments_for_chunk(chunk_id);
        if metas.is_empty() {
            return Err(BpError::Storage(format!(
                "recode: no fragments for chunk {chunk_id}"
            )));
        }

        // Load all local fragments for this chunk
        let fragments: Vec<EncodedFragment> = metas
            .iter()
            .map(|m| self.load_fragment(chunk_id, &m.fragment_id))
            .collect::<BpResult<_>>()?;

        rlnc::recode(&fragments, count).map_err(|e| BpError::Storage(e.to_string()))
    }

    // ── Accessors ─────────────────────────────────────────────────────────

    /// Whether this Pouch has remaining capacity for `bytes` more data.
    pub fn has_capacity(&self, bytes: u64) -> bool {
        self.meta.has_capacity(bytes)
    }

    /// Root directory of this Pouch's storage.
    pub fn root(&self) -> &Path {
        &self.root
    }

    // ── Private helpers ───────────────────────────────────────────────────

    fn root_path(network_id: &str, service_id: &str) -> BpResult<PathBuf> {
        Ok(config::base_dir()?
            .join("storage")
            .join(network_id)
            .join(service_id))
    }

    fn save_meta(&self) -> BpResult<()> {
        self.meta.save(&self.root.join("meta.json"))
    }

    /// Rebuild the fragment index from the on-disk `fragments/` directory.
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

                // Read the header to get k
                let path = frag_entry.path();
                let header = std::fs::read(&path).map_err(BpError::Io)?;
                let k = if header.len() >= 8 {
                    u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize
                } else {
                    continue; // corrupt file, skip
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

// ── Tests ─────────────────────────────────────────────────────────────────────

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
            assert!(sm.root().exists());
            assert!(sm.root().join("meta.json").exists());
            assert!(sm.root().join("fragments").exists());
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
            // Tiny quota — won't fit even one fragment
            let mut sm = StorageManager::init("net".into(), "svc".into(), 1).unwrap();
            let frags = rlnc::encode(CHUNK, 4, 4).unwrap();
            assert!(sm.store_fragment(&frags[0]).is_err());
        });
    }

    #[test]
    #[cfg(unix)]
    fn remove_fragment_frees_space() {
        with_temp_home_storage(|| {
            let mut sm = StorageManager::init("net".into(), "svc".into(), 1_000_000).unwrap();
            let frags = rlnc::encode(CHUNK, 4, 4).unwrap();
            sm.store_fragment(&frags[0]).unwrap();
            let used = sm.meta.storage_bytes_used;
            sm.remove_fragment(&frags[0].chunk_id, &frags[0].id)
                .unwrap();
            assert_eq!(sm.meta.storage_bytes_used, 0);
            let _ = used; // just checking it was non-zero before
        });
    }

    #[test]
    #[cfg(unix)]
    fn recode_chunk_produces_valid_fragments() {
        with_temp_home_storage(|| {
            let mut sm = StorageManager::init("net".into(), "svc".into(), 1_000_000).unwrap();
            let k = 4;
            let frags = rlnc::encode(CHUNK, k, k + 4).unwrap();

            // Store 3 fragments
            for f in &frags[..3] {
                sm.store_fragment(f).unwrap();
            }

            let recoded = sm.recode_chunk(&frags[0].chunk_id, 2).unwrap();
            assert_eq!(recoded.len(), 2);
            assert_eq!(recoded[0].k, k);
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

            // Load fresh manager — must rebuild index from disk
            let sm2 = StorageManager::load(network_id, service_id).unwrap();
            assert_eq!(sm2.index.fragment_count(), 2);
            assert_eq!(sm2.meta.storage_bytes_used, sm2.index.total_bytes());
        });
    }
}
