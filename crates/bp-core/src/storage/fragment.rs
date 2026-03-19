//! In-memory index of fragments held by a Pouch.
//!
//! The `FragmentIndex` is rebuilt at daemon startup by scanning the on-disk
//! `fragments/` directory and is kept in sync with every store/remove operation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Lightweight record of a single fragment stored on disk.
///
/// Does not hold the fragment data — only enough information for the quality
/// monitor to issue proof-of-storage challenges and for the storage manager
/// to locate the file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentMeta {
    /// UUID of the fragment (also the filename without extension).
    pub fragment_id: String,
    /// BLAKE3 chunk hash prefix this fragment belongs to.
    pub chunk_id: String,
    /// Number of source symbols `k` for this fragment's chunk.
    pub k: usize,
    /// On-disk size in bytes (full binary blob including header).
    pub size_bytes: u64,
}

/// In-memory index: `chunk_id → Vec<FragmentMeta>`.
///
/// Keyed by chunk_id so that the storage manager can quickly find all
/// fragments of a given chunk (for recoding or proof-of-storage responses).
#[derive(Debug, Default)]
pub struct FragmentIndex {
    /// chunk_id → list of fragment metadata records.
    inner: HashMap<String, Vec<FragmentMeta>>,
    /// Total bytes tracked by this index (sum of fragment sizes).
    total_bytes: u64,
}

impl FragmentIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new fragment.
    pub fn insert(&mut self, meta: FragmentMeta) {
        self.total_bytes += meta.size_bytes;
        self.inner
            .entry(meta.chunk_id.clone())
            .or_default()
            .push(meta);
    }

    /// Remove a fragment by `chunk_id` + `fragment_id`.
    /// Returns the removed entry if found.
    pub fn remove(&mut self, chunk_id: &str, fragment_id: &str) -> Option<FragmentMeta> {
        let entries = self.inner.get_mut(chunk_id)?;
        let pos = entries.iter().position(|m| m.fragment_id == fragment_id)?;
        let removed = entries.swap_remove(pos);
        self.total_bytes = self.total_bytes.saturating_sub(removed.size_bytes);
        if entries.is_empty() {
            self.inner.remove(chunk_id);
        }
        Some(removed)
    }

    /// All fragment records for a given chunk (for recoding / PoS challenges).
    pub fn fragments_for_chunk(&self, chunk_id: &str) -> &[FragmentMeta] {
        self.inner
            .get(chunk_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// All chunk IDs tracked.
    pub fn chunk_ids(&self) -> impl Iterator<Item = &str> {
        self.inner.keys().map(String::as_str)
    }

    /// Total number of fragments across all chunks.
    pub fn fragment_count(&self) -> usize {
        self.inner.values().map(Vec::len).sum()
    }

    /// Total bytes occupied by all stored fragments.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Whether the index holds any fragment for the given chunk.
    pub fn contains_chunk(&self, chunk_id: &str) -> bool {
        self.inner.contains_key(chunk_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_meta(chunk: &str, frag: &str, size: u64) -> FragmentMeta {
        FragmentMeta {
            fragment_id: frag.into(),
            chunk_id: chunk.into(),
            k: 4,
            size_bytes: size,
        }
    }

    #[test]
    fn insert_and_query() {
        let mut idx = FragmentIndex::new();
        idx.insert(make_meta("chunk1", "frag-a", 1024));
        idx.insert(make_meta("chunk1", "frag-b", 1024));
        idx.insert(make_meta("chunk2", "frag-c", 2048));

        assert_eq!(idx.fragment_count(), 3);
        assert_eq!(idx.total_bytes(), 4096);
        assert_eq!(idx.fragments_for_chunk("chunk1").len(), 2);
        assert_eq!(idx.fragments_for_chunk("chunk2").len(), 1);
        assert_eq!(idx.fragments_for_chunk("chunk3").len(), 0);
    }

    #[test]
    fn remove_existing() {
        let mut idx = FragmentIndex::new();
        idx.insert(make_meta("c1", "f1", 500));
        idx.insert(make_meta("c1", "f2", 500));

        let removed = idx.remove("c1", "f1");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().fragment_id, "f1");
        assert_eq!(idx.fragment_count(), 1);
        assert_eq!(idx.total_bytes(), 500);
    }

    #[test]
    fn remove_last_fragment_cleans_chunk_key() {
        let mut idx = FragmentIndex::new();
        idx.insert(make_meta("c1", "f1", 100));
        idx.remove("c1", "f1");
        assert!(!idx.contains_chunk("c1"));
        assert_eq!(idx.fragment_count(), 0);
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut idx = FragmentIndex::new();
        assert!(idx.remove("no-chunk", "no-frag").is_none());
    }
}
