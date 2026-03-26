//! Persistent catalogue of files uploaded by this node.
//!
//! Every successful `bp put` call appends a [`StoredFileEntry`] to the
//! local registry so that `bp ls` can enumerate all uploaded files without
//! re-contacting the network.
//!
//! The registry is stored as a flat JSON array in
//! `~/.local/share/billpouch/file_registry.json`.

use crate::error::{BpError, BpResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── StoredFileEntry ───────────────────────────────────────────────────────────

/// Metadata for a single file uploaded by this node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFileEntry {
    /// Display name of the file (filename or user-supplied alias).
    pub file_name: String,
    /// Original size in bytes (before chunking / encryption).
    pub size_bytes: u64,
    /// BLAKE3 chunk_id returned by `PutFile`.
    pub chunk_id: String,
    /// Network the file was uploaded to.
    pub network_id: String,
    /// Unix timestamp (seconds) when the file was uploaded.
    pub uploaded_at: u64,
}

// ── FileRegistry ──────────────────────────────────────────────────────────────

/// In-memory catalogue backed by `file_registry.json`.
#[derive(Debug, Default)]
pub struct FileRegistry {
    entries: Vec<StoredFileEntry>,
}

impl FileRegistry {
    /// Create an empty registry (not persisted until [`save`] is called).
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from `path`, returning an empty registry if the file is missing
    /// or cannot be parsed (never fails hard).
    pub fn load(path: &Path) -> Self {
        let entries = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<StoredFileEntry>>(&s).ok())
            .unwrap_or_default();
        Self { entries }
    }

    /// Persist to `path` as a pretty-printed JSON array.
    pub fn save(&self, path: &Path) -> BpResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(BpError::Io)?;
        }
        let json = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(path, json).map_err(BpError::Io)
    }

    /// Append a new entry and immediately persist.
    ///
    /// Persistence failures are logged as warnings and not propagated so
    /// that a registry write failure never aborts a successful `PutFile`.
    pub fn insert_and_save(&mut self, entry: StoredFileEntry, path: &Path) {
        self.entries.push(entry);
        if let Err(e) = self.save(path) {
            tracing::warn!("FileRegistry: failed to persist: {e}");
        }
    }

    /// Return all entries, optionally filtered by `network_id`.
    ///
    /// Pass an empty string to get all entries across networks.
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

    /// Total bytes of all uploaded files (across all networks).
    pub fn total_uploaded_bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.size_bytes).sum()
    }

    /// Total number of registered files.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return `true` if no files have been uploaded yet.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
