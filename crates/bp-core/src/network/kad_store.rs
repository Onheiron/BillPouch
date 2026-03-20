//! Persistence for the Kademlia routing table.
//!
//! On every save tick (600 s) and on daemon shutdown, the set of peer
//! multi-addresses discovered via mDNS or Identify is written to
//! `~/.local/share/billpouch/kad_peers.json`.
//!
//! On the next daemon startup, [`KadPeers::load`] reads that file and the
//! network loop dials each saved address.  This lets the node reconnect to
//! previously-known peers instantly, without waiting for mDNS multicast.
//!
//! ## File format
//!
//! Plain JSON object mapping `peer_id_string → [multiaddr_string, ...]`.
//!
//! ```json
//! {
//!   "12D3KooW...": ["/ip4/192.168.1.10/tcp/41000", "/ip6/.../tcp/41001"],
//!   ...
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

// ── KadPeers ──────────────────────────────────────────────────────────────────

/// Persisted map of `peer_id → [multiaddr_string, ...]`.
///
/// Wraps a plain [`HashMap`] so we can implement load/save once and reuse the
/// type in the network loop.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KadPeers(pub HashMap<String, Vec<String>>);

impl KadPeers {
    /// Load from `path`.  Returns an empty [`KadPeers`] if the file does not
    /// exist or cannot be parsed.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Write to `path` as pretty-printed JSON.  Logs a warning on failure
    /// (never panics).
    pub fn save(&self, path: &Path) {
        match serde_json::to_string_pretty(self) {
            Ok(s) => {
                if let Err(e) = std::fs::write(path, s) {
                    tracing::warn!("kad_store: failed to save peers: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!("kad_store: serialization error: {}", e);
            }
        }
    }

    /// Add `addr_str` to the entry for `peer_id_str`, deduplicating in place.
    pub fn add(&mut self, peer_id_str: String, addr_str: String) {
        let entry = self.0.entry(peer_id_str).or_default();
        if !entry.contains(&addr_str) {
            entry.push(addr_str);
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("bp_kad_test_{name}.json"))
    }

    #[test]
    fn round_trip_empty() {
        let path = tmp_path("empty");
        let peers = KadPeers::default();
        peers.save(&path);
        let loaded = KadPeers::load(&path);
        let _ = std::fs::remove_file(&path);
        assert!(loaded.0.is_empty());
    }

    #[test]
    fn round_trip_with_entries() {
        let path = tmp_path("entries");
        let mut peers = KadPeers::default();
        peers.add("peer-1".into(), "/ip4/1.2.3.4/tcp/4001".into());
        peers.add("peer-1".into(), "/ip6/::1/tcp/4001".into());
        peers.add("peer-2".into(), "/ip4/5.6.7.8/tcp/4002".into());
        peers.save(&path);

        let loaded = KadPeers::load(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.0["peer-1"].len(), 2);
        assert_eq!(loaded.0["peer-2"].len(), 1);
    }

    #[test]
    fn add_deduplicates() {
        let mut peers = KadPeers::default();
        peers.add("p1".into(), "/ip4/1.2.3.4/tcp/4001".into());
        peers.add("p1".into(), "/ip4/1.2.3.4/tcp/4001".into());
        assert_eq!(peers.0["p1"].len(), 1);
    }

    #[test]
    fn load_missing_file_returns_default() {
        let peers = KadPeers::load(std::path::Path::new("/tmp/no_such_kad_file_xyz.json"));
        assert!(peers.0.is_empty());
    }

    #[test]
    fn load_corrupt_file_returns_default() {
        let path = tmp_path("corrupt");
        std::fs::write(&path, b"NOT VALID JSON {").unwrap();
        let peers = KadPeers::load(&path);
        let _ = std::fs::remove_file(&path);
        assert!(peers.0.is_empty());
    }
}
