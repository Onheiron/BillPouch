use libp2p::{multiaddr::Protocol, Multiaddr, PeerId, Swarm};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::network::behaviour::BillPouchBehaviour;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BootstrapList(pub Vec<String>);

impl BootstrapList {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        match serde_json::to_string_pretty(self) {
            Ok(s) => {
                if let Err(e) = std::fs::write(path, s) {
                    tracing::warn!("bootstrap: failed to save list: {e}");
                }
            }
            Err(e) => tracing::warn!("bootstrap: serialization error: {e}"),
        }
    }

    pub fn add(&mut self, addr: String) {
        if !self.0.contains(&addr) {
            self.0.push(addr);
        }
    }

    pub fn remove(&mut self, addr: &str) {
        self.0.retain(|a| a != addr);
    }

    pub fn apply(&self, swarm: &mut Swarm<BillPouchBehaviour>) {
        if self.0.is_empty() {
            return;
        }
        tracing::info!(n = self.0.len(), "bootstrap: applying bootstrap peer list");
        for addr_str in &self.0 {
            match addr_str.parse::<Multiaddr>() {
                Ok(addr) => match peer_id_from_multiaddr(&addr) {
                    Some(peer_id) => {
                        swarm
                            .behaviour_mut()
                            .kad
                            .add_address(&peer_id, addr.clone());
                        swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                        if let Err(e) = swarm.dial(addr.clone()) {
                            tracing::warn!(addr = %addr_str, "bootstrap: dial failed: {e}");
                        } else {
                            tracing::debug!(addr = %addr_str, peer = %peer_id, "bootstrap: dialing");
                        }
                    }
                    None => {
                        tracing::warn!(
                            addr = %addr_str,
                            "bootstrap: no /p2p/<PeerId> in address, skipping"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(addr = %addr_str, "bootstrap: invalid multiaddr: {e}");
                }
            }
        }
    }
}

fn peer_id_from_multiaddr(addr: &Multiaddr) -> Option<PeerId> {
    addr.iter().find_map(|proto| {
        if let Protocol::P2p(peer_id) = proto {
            Some(peer_id)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("bp_bootstrap_test_{name}.json"))
    }

    #[test]
    fn load_missing_returns_default() {
        let list = BootstrapList::load(Path::new("/tmp/no_such_bootstrap_xyz.json"));
        assert!(list.0.is_empty());
    }

    #[test]
    fn load_corrupt_returns_default() {
        let path = tmp_path("corrupt");
        std::fs::write(&path, b"NOT JSON{{").unwrap();
        let list = BootstrapList::load(&path);
        let _ = std::fs::remove_file(&path);
        assert!(list.0.is_empty());
    }

    #[test]
    fn round_trip() {
        let path = tmp_path("roundtrip");
        let mut list = BootstrapList::default();
        list.add("/ip4/1.2.3.4/tcp/4001/p2p/12D3KooWFakeAAA".into());
        list.add("/ip4/5.6.7.8/tcp/4001/p2p/12D3KooWFakeBBB".into());
        list.save(&path);

        let loaded = BootstrapList::load(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.0.len(), 2);
    }

    #[test]
    fn add_deduplicates() {
        let mut list = BootstrapList::default();
        list.add("addr1".into());
        list.add("addr1".into());
        assert_eq!(list.0.len(), 1);
    }

    #[test]
    fn remove_works() {
        let mut list = BootstrapList::default();
        list.add("addr1".into());
        list.add("addr2".into());
        list.remove("addr1");
        assert_eq!(list.0, vec!["addr2".to_string()]);
    }

    #[test]
    fn peer_id_from_multiaddr_with_p2p() {
        let addr_no_p2p: Multiaddr = "/ip4/1.2.3.4/tcp/4001".parse().unwrap();
        assert!(peer_id_from_multiaddr(&addr_no_p2p).is_none());
    }
}
