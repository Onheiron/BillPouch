use crate::service::ServiceType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub peer_id: String,
    pub user_fingerprint: String,
    pub user_alias: Option<String>,
    pub service_type: ServiceType,
    pub service_id: String,
    pub network_id: String,
    pub listen_addrs: Vec<String>,
    pub announced_at: u64,
    pub metadata: HashMap<String, serde_json::Value>,
}

impl NodeInfo {
    pub fn topic_name(network_id: &str) -> String {
        format!("billpouch/v1/{}/nodes", network_id)
    }
}

#[derive(Default)]
pub struct NetworkState {
    nodes: HashMap<String, NodeInfo>,
}

impl NetworkState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&mut self, info: NodeInfo) {
        let entry = self
            .nodes
            .entry(info.peer_id.clone())
            .or_insert_with(|| info.clone());
        if info.announced_at >= entry.announced_at {
            *entry = info;
        }
    }

    pub fn remove(&mut self, peer_id: &str) {
        self.nodes.remove(peer_id);
    }

    pub fn all(&self) -> Vec<&NodeInfo> {
        self.nodes.values().collect()
    }

    pub fn in_network<'a>(&'a self, network_id: &str) -> Vec<&'a NodeInfo> {
        self.nodes
            .values()
            .filter(|n| n.network_id == network_id)
            .collect()
    }

    pub fn evict_stale(&mut self, max_age_secs: u64) {
        let now = chrono::Utc::now().timestamp() as u64;
        self.nodes
            .retain(|_, n| now.saturating_sub(n.announced_at) < max_age_secs);
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
