use crate::{identity::DeviceCert, service::ServiceType};
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
    pub device_cert: DeviceCert,
}

impl NodeInfo {
    pub fn is_authentic(&self, publisher: Option<&libp2p::PeerId>) -> bool {
        let Ok(device) = self.device_cert.verify() else {
            return false;
        };
        let peer = device.peer_id.to_string();
        peer == self.peer_id
            && device.fingerprint == self.user_fingerprint
            && publisher.map(|p| p.to_string()) == Some(peer)
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

    pub fn knows(&self, peer_id: &str) -> bool {
        self.nodes.contains_key(peer_id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    fn announce(id: &Identity) -> NodeInfo {
        NodeInfo {
            peer_id: id.peer_id.to_string(),
            user_fingerprint: id.fingerprint.clone(),
            user_alias: None,
            service_type: ServiceType::Pouch,
            service_id: "svc".into(),
            network_id: "amici".into(),
            listen_addrs: vec![],
            announced_at: 0,
            metadata: HashMap::new(),
            device_cert: id.cert.clone(),
        }
    }

    #[test]
    fn genuine_announcement_is_accepted() {
        let id = Identity::in_memory(None).unwrap();
        assert!(announce(&id).is_authentic(Some(&id.peer_id)));
    }

    #[test]
    fn forged_announcements_are_rejected() {
        let carlo = Identity::in_memory(None).unwrap();
        let mallory = Identity::in_memory(None).unwrap();

        assert!(!announce(&carlo).is_authentic(Some(&mallory.peer_id)));
        assert!(!announce(&carlo).is_authentic(None));

        let mut impersonation = announce(&mallory);
        impersonation.user_fingerprint = carlo.fingerprint.clone();
        assert!(!impersonation.is_authentic(Some(&mallory.peer_id)));

        let mut stolen_cert = announce(&mallory);
        stolen_cert.device_cert = carlo.cert.clone();
        assert!(!stolen_cert.is_authentic(Some(&mallory.peer_id)));
    }
}
