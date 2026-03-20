//! Fragment-index gossip — distributed discovery of which Pouch holds which fragment.
//!
//! ## Purpose
//!
//! When a Bill node runs `PutFile` and distributes RLNC fragments to remote
//! Pouch peers, it publishes a [`FragmentIndexAnnouncement`] on a dedicated
//! gossipsub topic.  All nodes that receive the announcement update their
//! local [`RemoteFragmentIndex`], so they know exactly which Pouch holds
//! each fragment of a chunk.
//!
//! This enables **targeted** `FetchChunk` requests:
//! instead of broadcasting a [`crate::network::NetworkCommand::FetchChunkFragments`]
//! to every Pouch in the network, `GetFile` can send a
//! [`crate::network::NetworkCommand::FetchChunkFragments`] only to the peers
//! listed in [`RemoteFragmentIndex::pointers_for`].
//!
//! ## Topic
//!
//! `billpouch/v1/{network_id}/index`
//!
//! Separate from the NodeInfo topic (`billpouch/v1/{network_id}/nodes`) so
//! that consumers can subscribe selectively.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Message types ─────────────────────────────────────────────────────────────

/// A gossipped pointer: which `peer_id` (Pouch) holds `fragment_id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FragmentPointer {
    /// libp2p PeerId (base58) of the Pouch that stores the fragment.
    pub peer_id: String,
    /// UUID of the specific RLNC fragment within the chunk.
    pub fragment_id: String,
}

/// Gossip announcement published after a `PutFile` distributes fragments.
///
/// Published on [`FragmentIndexAnnouncement::topic_name`] by the Bill node
/// that executed the distribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentIndexAnnouncement {
    /// Network this chunk belongs to.
    pub network_id: String,
    /// BLAKE3-derived chunk identifier (same as `chunk_id` in `PutFile` response).
    pub chunk_id: String,
    /// Unix timestamp (seconds) of this announcement.
    pub announced_at: u64,
    /// One entry per distributed fragment: which Pouch holds it.
    pub pointers: Vec<FragmentPointer>,
}

impl FragmentIndexAnnouncement {
    /// Gossipsub topic name for the fragment index of `network_id`.
    ///
    /// Format: `billpouch/v1/{network_id}/index`
    pub fn topic_name(network_id: &str) -> String {
        format!("billpouch/v1/{}/index", network_id)
    }
}

// ── RemoteFragmentIndex ───────────────────────────────────────────────────────

/// In-memory index accumulated from received [`FragmentIndexAnnouncement`]s.
///
/// Maps `chunk_id → Vec<FragmentPointer>`.
///
/// Held in [`crate::control::server::DaemonState`] under an `Arc<RwLock<>>`.
#[derive(Debug, Default)]
pub struct RemoteFragmentIndex {
    /// chunk_id → list of (peer_id, fragment_id) pointers.
    inner: HashMap<String, Vec<FragmentPointer>>,
}

impl RemoteFragmentIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Incorporate a [`FragmentIndexAnnouncement`].
    ///
    /// Merges new pointers into the existing list for the chunk, de-duplicating
    /// by `fragment_id` so repeated announcements are idempotent.
    pub fn upsert(&mut self, ann: FragmentIndexAnnouncement) {
        let entry = self.inner.entry(ann.chunk_id).or_default();
        for ptr in ann.pointers {
            if !entry.iter().any(|e| e.fragment_id == ptr.fragment_id) {
                entry.push(ptr);
            }
        }
    }

    /// All known fragment pointers for a chunk.  Returns an empty slice if
    /// the chunk has never been announced.
    pub fn pointers_for(&self, chunk_id: &str) -> &[FragmentPointer] {
        self.inner
            .get(chunk_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Pointers for a chunk hosted by a specific peer.
    pub fn pointers_for_peer<'a>(
        &'a self,
        chunk_id: &str,
        peer_id: &str,
    ) -> impl Iterator<Item = &'a FragmentPointer> {
        self.pointers_for(chunk_id)
            .iter()
            .filter(move |p| p.peer_id == peer_id)
    }

    /// Remove all pointer entries for a given `peer_id` (e.g. after it is
    /// blacklisted or evicted from the network).
    pub fn evict_peer(&mut self, peer_id: &str) {
        for ptrs in self.inner.values_mut() {
            ptrs.retain(|p| p.peer_id != peer_id);
        }
        self.inner.retain(|_, v| !v.is_empty());
    }

    /// Total number of chunks tracked.
    pub fn chunk_count(&self) -> usize {
        self.inner.len()
    }

    /// Total number of fragment pointers across all chunks.
    pub fn pointer_count(&self) -> usize {
        self.inner.values().map(Vec::len).sum()
    }

    /// All chunk IDs for which at least one pointer is known.
    pub fn chunk_ids(&self) -> impl Iterator<Item = &str> {
        self.inner.keys().map(String::as_str)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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
        idx.upsert(make_ann("chunk-1", &[("peer-a", "frag-1"), ("peer-b", "frag-2")]));
        let ptrs = idx.pointers_for("chunk-1");
        assert_eq!(ptrs.len(), 2);
        assert_eq!(idx.chunk_count(), 1);
        assert_eq!(idx.pointer_count(), 2);
    }

    #[test]
    fn upsert_is_idempotent() {
        let mut idx = RemoteFragmentIndex::new();
        idx.upsert(make_ann("chunk-1", &[("peer-a", "frag-1")]));
        idx.upsert(make_ann("chunk-1", &[("peer-a", "frag-1")])); // duplicate
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
    fn evict_peer_removes_entries() {
        let mut idx = RemoteFragmentIndex::new();
        idx.upsert(make_ann(
            "chunk-1",
            &[("peer-a", "frag-1"), ("peer-b", "frag-2")],
        ));
        idx.evict_peer("peer-a");
        let ptrs = idx.pointers_for("chunk-1");
        assert_eq!(ptrs.len(), 1);
        assert_eq!(ptrs[0].peer_id, "peer-b");
    }

    #[test]
    fn evict_last_peer_removes_chunk() {
        let mut idx = RemoteFragmentIndex::new();
        idx.upsert(make_ann("chunk-1", &[("peer-a", "frag-1")]));
        idx.evict_peer("peer-a");
        assert_eq!(idx.chunk_count(), 0);
    }

    #[test]
    fn pointers_for_unknown_chunk_is_empty() {
        let idx = RemoteFragmentIndex::new();
        assert!(idx.pointers_for("unknown").is_empty());
    }

    #[test]
    fn pointers_for_peer_filters_correctly() {
        let mut idx = RemoteFragmentIndex::new();
        idx.upsert(make_ann(
            "chunk-1",
            &[("peer-a", "frag-1"), ("peer-b", "frag-2"), ("peer-a", "frag-3")],
        ));
        let count = idx.pointers_for_peer("chunk-1", "peer-a").count();
        assert_eq!(count, 2);
    }
}
