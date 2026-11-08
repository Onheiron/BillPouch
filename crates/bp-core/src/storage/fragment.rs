use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentMeta {
    pub fragment_id: String,
    pub chunk_id: String,
    pub k: usize,
    pub size_bytes: u64,
}

#[derive(Debug, Default)]
pub struct FragmentIndex {
    inner: HashMap<String, Vec<FragmentMeta>>,
    total_bytes: u64,
}

impl FragmentIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, meta: FragmentMeta) {
        self.total_bytes += meta.size_bytes;
        self.inner
            .entry(meta.chunk_id.clone())
            .or_default()
            .push(meta);
    }

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

    pub fn remove_chunk(&mut self, chunk_id: &str) -> u64 {
        let freed: u64 = self
            .inner
            .remove(chunk_id)
            .map_or(0, |m| m.iter().map(|f| f.size_bytes).sum());
        self.total_bytes = self.total_bytes.saturating_sub(freed);
        freed
    }

    pub fn fragments_for_chunk(&self, chunk_id: &str) -> &[FragmentMeta] {
        self.inner
            .get(chunk_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn chunk_ids(&self) -> impl Iterator<Item = &str> {
        self.inner.keys().map(String::as_str)
    }

    pub fn fragment_count(&self) -> usize {
        self.inner.values().map(Vec::len).sum()
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
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
        assert_eq!(idx.fragment_count(), 0);
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut idx = FragmentIndex::new();
        assert!(idx.remove("no-chunk", "no-frag").is_none());
    }
}
