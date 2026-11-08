use crate::error::{BpError, BpResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PouchMeta {
    pub network_id: String,
    pub service_id: String,
    pub tier: super::StorageTier,
    pub storage_bytes_bid: u64,
    pub storage_bytes_used: u64,
    pub joined_at: u64,
}

impl PouchMeta {
    pub fn new(network_id: String, service_id: String, storage_bytes_bid: u64) -> Self {
        Self {
            network_id,
            service_id,
            tier: super::StorageTier::fitting(storage_bytes_bid),
            storage_bytes_bid,
            storage_bytes_used: 0,
            joined_at: chrono::Utc::now().timestamp() as u64,
        }
    }

    pub fn available_bytes(&self) -> u64 {
        self.storage_bytes_bid
            .saturating_sub(self.storage_bytes_used)
    }

    pub fn has_capacity(&self, bytes: u64) -> bool {
        self.available_bytes() >= bytes
    }

    pub fn load(path: &Path) -> BpResult<Self> {
        let json = std::fs::read_to_string(path).map_err(BpError::Io)?;
        serde_json::from_str(&json).map_err(BpError::Serde)
    }

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
