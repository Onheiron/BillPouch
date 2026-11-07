use crate::error::{BpError, BpResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFileEntry {
    pub file_name: String,
    pub size_bytes: u64,
    pub chunk_id: String,
    pub network_id: String,
    pub uploaded_at: u64,
}

#[derive(Debug, Default)]
pub struct FileRegistry {
    entries: Vec<StoredFileEntry>,
}

impl FileRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(path: &Path) -> Self {
        let entries = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<StoredFileEntry>>(&s).ok())
            .unwrap_or_default();
        Self { entries }
    }

    pub fn save(&self, path: &Path) -> BpResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(BpError::Io)?;
        }
        let json = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(path, json).map_err(BpError::Io)
    }

    pub fn insert_and_save(&mut self, entry: StoredFileEntry, path: &Path) {
        self.entries.push(entry);
        if let Err(e) = self.save(path) {
            tracing::warn!("FileRegistry: failed to persist: {e}");
        }
    }

    pub fn list(&self, network_id: &str) -> Vec<&StoredFileEntry> {
        if network_id.is_empty() {
            self.entries.iter().collect()
        } else {
            self.entries
                .iter()
                .filter(|e| e.network_id == network_id)
                .collect()
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
