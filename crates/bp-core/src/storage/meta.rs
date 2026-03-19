//! `PouchMeta` — on-disk metadata for a Pouch storage bid.
//!
//! Serialised as `meta.json` inside
//! `~/.local/share/billpouch/storage/<network_id>/<service_id>/meta.json`.

use crate::error::{BpError, BpResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Persistent metadata for a Pouch storage bid.
///
/// Written when the Pouch service is first hatched and updated whenever
/// a fragment is stored or removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PouchMeta {
    /// Network this Pouch is participating in.
    pub network_id: String,
    /// UUID of the Pouch service instance.
    pub service_id: String,
    /// Bytes offered to the network at bid time.
    pub storage_bytes_bid: u64,
    /// Bytes currently occupied by stored fragments.
    pub storage_bytes_used: u64,
    /// Unix timestamp (seconds) when this Pouch joined.
    pub joined_at: u64,
}

impl PouchMeta {
    /// Create a new meta record with `storage_bytes_used = 0`.
    pub fn new(network_id: String, service_id: String, storage_bytes_bid: u64) -> Self {
        Self {
            network_id,
            service_id,
            storage_bytes_bid,
            storage_bytes_used: 0,
            joined_at: chrono::Utc::now().timestamp() as u64,
        }
    }

    /// Bytes still available for new fragments.
    pub fn available_bytes(&self) -> u64 {
        self.storage_bytes_bid
            .saturating_sub(self.storage_bytes_used)
    }

    /// Whether the Pouch can accept `bytes` more data.
    pub fn has_capacity(&self, bytes: u64) -> bool {
        self.available_bytes() >= bytes
    }

    /// Load from a `meta.json` file.
    pub fn load(path: &Path) -> BpResult<Self> {
        let json = std::fs::read_to_string(path).map_err(BpError::Io)?;
        serde_json::from_str(&json).map_err(BpError::Serde)
    }

    /// Persist to a `meta.json` file (creates or overwrites).
    pub fn save(&self, path: &Path) -> BpResult<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).map_err(BpError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_bytes_correct() {
        let mut m = PouchMeta::new("net".into(), "svc".into(), 1000);
        assert_eq!(m.available_bytes(), 1000);
        m.storage_bytes_used = 400;
        assert_eq!(m.available_bytes(), 600);
    }

    #[test]
    fn has_capacity_boundary() {
        let mut m = PouchMeta::new("net".into(), "svc".into(), 100);
        assert!(m.has_capacity(100));
        assert!(!m.has_capacity(101));
        m.storage_bytes_used = 50;
        assert!(m.has_capacity(50));
        assert!(!m.has_capacity(51));
    }

    #[test]
    #[cfg(unix)]
    fn save_and_load_roundtrip() {
        let dir =
            std::env::temp_dir().join(format!("bp_meta_test_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("meta.json");

        let meta = PouchMeta::new("amici".into(), "svc-uuid".into(), 10_737_418_240);
        meta.save(&path).unwrap();

        let loaded = PouchMeta::load(&path).unwrap();
        assert_eq!(loaded.network_id, "amici");
        assert_eq!(loaded.storage_bytes_bid, 10_737_418_240);
        assert_eq!(loaded.storage_bytes_used, 0);

        std::fs::remove_dir_all(dir).ok();
    }
}
