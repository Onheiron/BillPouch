//! Distributed network state maintained via gossip.
//!
//! Each node in a BillPouch network periodically announces its `NodeInfo` over
//! gossipsub.  All nodes accumulate received announcements in a local
//! `NetworkState` map.  The `metadata` field on `NodeInfo` is intentionally
//! open (`HashMap<String, Value>`) so that future versions can extend the
//! gossipped information without a breaking protocol change.

use crate::service::ServiceType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Gossipped information about a single node (one peer = one service instance).
///
/// Multiple nodes can belong to the same user (same `user_fingerprint`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// libp2p PeerId (base58 string).
    pub peer_id: String,
    /// SHA-256(pubkey)[0..8] hex — ties multiple nodes to one user.
    pub user_fingerprint: String,
    /// Optional alias chosen at login.
    pub user_alias: Option<String>,
    pub service_type: ServiceType,
    /// UUID of the local service instance.
    pub service_id: String,
    /// Network this node belongs to.
    pub network_id: String,
    /// Multiaddrs the node is listening on.
    pub listen_addrs: Vec<String>,
    /// Unix timestamp (seconds) when this announcement was created.
    pub announced_at: u64,
    /// Extensible key-value metadata (storage size, capabilities, …).
    pub metadata: HashMap<String, serde_json::Value>,
}

impl NodeInfo {
    /// Gossipsub topic name for the given network.
    pub fn topic_name(network_id: &str) -> String {
        format!("billpouch/v1/{}/nodes", network_id)
    }
}

/// Local in-memory view of all known nodes in all joined networks.
#[derive(Default)]
pub struct NetworkState {
    /// peer_id (string) → latest NodeInfo
    nodes: HashMap<String, NodeInfo>,
}

impl NetworkState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert (or insert) a `NodeInfo`, keeping the newest announcement.
    pub fn upsert(&mut self, info: NodeInfo) {
        let entry = self.nodes.entry(info.peer_id.clone()).or_insert_with(|| info.clone());
        if info.announced_at >= entry.announced_at {
            *entry = info;
        }
    }

    /// Remove a node by PeerId string.
    pub fn remove(&mut self, peer_id: &str) {
        self.nodes.remove(peer_id);
    }

    /// All known nodes.
    pub fn all(&self) -> Vec<&NodeInfo> {
        self.nodes.values().collect()
    }

    /// All nodes in a specific network.
    pub fn in_network<'a>(&'a self, network_id: &str) -> Vec<&'a NodeInfo> {
        self.nodes
            .values()
            .filter(|n| n.network_id == network_id)
            .collect()
    }

    /// Evict nodes whose last announcement is older than `max_age_secs`.
    pub fn evict_stale(&mut self, max_age_secs: u64) {
        let now = chrono::Utc::now().timestamp() as u64;
        self.nodes.retain(|_, n| now.saturating_sub(n.announced_at) < max_age_secs);
    }

    /// Total node count.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
}
