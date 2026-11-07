use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FragmentPointer {
    pub peer_id: String,
    pub fragment_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentIndexAnnouncement {
    pub network_id: String,
    pub chunk_id: String,
    pub announced_at: u64,
    pub pointers: Vec<FragmentPointer>,
}

impl FragmentIndexAnnouncement {
    pub fn topic_name(network_id: &str) -> String {
        format!("billpouch/v1/{}/index", network_id)
    }
}

#[derive(Debug, Default)]
pub struct RemoteFragmentIndex {
    inner: HashMap<String, Vec<FragmentPointer>>,
}

impl RemoteFragmentIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&mut self, ann: FragmentIndexAnnouncement) {
        let entry = self.inner.entry(ann.chunk_id).or_default();
        for ptr in ann.pointers {
            if !entry.iter().any(|e| e.fragment_id == ptr.fragment_id) {
                entry.push(ptr);
            }
        }
    }

    pub fn pointers_for(&self, chunk_id: &str) -> &[FragmentPointer] {
        self.inner
            .get(chunk_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn chunk_ids(&self) -> impl Iterator<Item = &str> {
        self.inner.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ann(chunk: &str, pointers: &[(&str, &str)]) -> FragmentIndexAnnouncement {
        FragmentIndexAnnouncement {
            network_id: "test".into(),
            chunk_id: chunk.into(),
            announced_at: 0,
            pointers: pointers
                .iter()
                .map(|(peer, frag)| FragmentPointer {
                    peer_id: (*peer).into(),
                    fragment_id: (*frag).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn topic_name_format() {
        assert_eq!(
            FragmentIndexAnnouncement::topic_name("amici"),
            "billpouch/v1/amici/index"
        );
    }

    #[test]
    fn upsert_and_query() {
        let mut idx = RemoteFragmentIndex::new();
        idx.upsert(make_ann(
            "chunk-1",
            &[("peer-a", "frag-1"), ("peer-b", "frag-2")],
        ));
        let ptrs = idx.pointers_for("chunk-1");
        assert_eq!(ptrs.len(), 2);
    }

    #[test]
    fn upsert_is_idempotent() {
        let mut idx = RemoteFragmentIndex::new();
        idx.upsert(make_ann("chunk-1", &[("peer-a", "frag-1")]));
        idx.upsert(make_ann("chunk-1", &[("peer-a", "frag-1")]));
        assert_eq!(idx.pointers_for("chunk-1").len(), 1);
    }

    #[test]
    fn upsert_merges_new_frags() {
        let mut idx = RemoteFragmentIndex::new();
        idx.upsert(make_ann("chunk-1", &[("peer-a", "frag-1")]));
        idx.upsert(make_ann("chunk-1", &[("peer-b", "frag-2")]));
        assert_eq!(idx.pointers_for("chunk-1").len(), 2);
    }

    #[test]
    fn pointers_for_unknown_chunk_is_empty() {
        let idx = RemoteFragmentIndex::new();
        assert!(idx.pointers_for("unknown").is_empty());
    }
}
