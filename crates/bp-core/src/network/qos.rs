use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const ALPHA_RTT: f64 = 0.125;

const BETA_CHALLENGE: f64 = 0.10;

const MEMORY: f64 = 0.9995;

const PRUDENCE_Z: f64 = 1.645;

pub fn availability_lower_bound(seen: f64, missed: f64) -> f64 {
    let (a, b) = (1.0 + seen, 1.0 + missed);
    let total = a + b;
    let mean = a / total;
    let spread = (a * b / (total * total * (total + 1.0))).sqrt();
    (mean - PRUDENCE_Z * spread).clamp(0.0, 1.0)
}

pub fn unknown_availability() -> f64 {
    availability_lower_bound(0.0, 0.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerQos {
    pub peer_id: String,

    pub rtt_ewma_ms: Option<f64>,

    pub challenge_success_ewma: f64,

    pub last_ping_at: Option<u64>,

    pub challenges_sent: u64,

    pub challenges_succeeded: u64,

    pub seen: f64,

    pub missed: f64,
}

impl PeerQos {
    pub fn new(peer_id: impl Into<String>) -> Self {
        Self {
            peer_id: peer_id.into(),
            rtt_ewma_ms: None,
            challenge_success_ewma: 1.0,
            last_ping_at: None,
            challenges_sent: 0,
            challenges_succeeded: 0,
            seen: 0.0,
            missed: 0.0,
        }
    }

    fn observe(&mut self, available: bool) {
        self.seen *= MEMORY;
        self.missed *= MEMORY;
        if available {
            self.seen += 1.0;
        } else {
            self.missed += 1.0;
        }
    }

    pub fn record_ping(&mut self, rtt_ms: f64) {
        self.rtt_ewma_ms = Some(match self.rtt_ewma_ms {
            None => rtt_ms,
            Some(prev) => ALPHA_RTT * rtt_ms + (1.0 - ALPHA_RTT) * prev,
        });
        self.last_ping_at = Some(chrono::Utc::now().timestamp() as u64);
        self.observe(true);
    }

    pub fn record_ping_timeout(&mut self) {
        self.observe(false);
    }

    pub fn record_challenge(&mut self, success: bool) {
        let outcome = if success { 1.0_f64 } else { 0.0_f64 };
        self.challenge_success_ewma =
            BETA_CHALLENGE * outcome + (1.0 - BETA_CHALLENGE) * self.challenge_success_ewma;
        self.challenges_sent += 1;
        if success {
            self.challenges_succeeded += 1;
        }
        self.observe(success);
    }

    pub fn availability(&self) -> f64 {
        availability_lower_bound(self.seen, self.missed)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct QosRegistry {
    peers: HashMap<String, PeerQos>,
}

impl QosRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load() -> Self {
        crate::config::qos_path()
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let result = crate::config::qos_path().and_then(|p| {
            std::fs::write(p, serde_json::to_vec(self)?)?;
            Ok(())
        });
        if let Err(e) = result {
            tracing::warn!("cannot save QoS: {e}");
        }
    }

    pub fn entry(&mut self, peer_id: impl Into<String>) -> &mut PeerQos {
        let key = peer_id.into();
        self.peers
            .entry(key.clone())
            .or_insert_with(|| PeerQos::new(key))
    }

    pub fn get(&self, peer_id: &str) -> Option<&PeerQos> {
        self.peers.get(peer_id)
    }

    pub fn remove(&mut self, peer_id: &str) {
        self.peers.remove(peer_id);
    }

    pub fn availability(&self, peer_id: &str) -> f64 {
        self.peers
            .get(peer_id)
            .map_or_else(unknown_availability, PeerQos::availability)
    }

    pub fn all_availabilities(&self) -> Vec<f64> {
        self.peers.values().map(PeerQos::availability).collect()
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn peer_ids(&self) -> impl Iterator<Item = &str> {
        self.peers.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_peer_counts_for_almost_nothing() {
        let p = PeerQos::new("peer1");
        assert!(p.availability() < 0.05, "{}", p.availability());
        assert!((p.availability() - unknown_availability()).abs() < 1e-12);
        assert_eq!(p.challenge_success_ewma, 1.0);
    }

    #[test]
    fn availability_grows_with_evidence_and_stays_below_the_truth() {
        let mut p = PeerQos::new("peer1");
        let mut last = p.availability();
        for _ in 0..30 {
            p.record_ping(10.0);
            assert!(p.availability() > last);
            last = p.availability();
        }
        for _ in 0..500 {
            p.record_ping(10.0);
        }
        assert!(
            p.availability() > 0.95 && p.availability() < 1.0,
            "{}",
            p.availability()
        );
    }

    #[test]
    fn misses_and_failed_challenges_lower_availability() {
        let mut p = PeerQos::new("peer1");
        for _ in 0..100 {
            p.record_ping(10.0);
        }
        let before = p.availability();
        for _ in 0..10 {
            p.record_ping_timeout();
        }
        let after_timeouts = p.availability();
        assert!(after_timeouts < before);
        for _ in 0..10 {
            p.record_challenge(false);
        }
        assert!(p.availability() < after_timeouts);
    }

    #[test]
    fn a_half_present_peer_is_estimated_below_half() {
        let mut p = PeerQos::new("peer1");
        for i in 0..400 {
            if i % 2 == 0 {
                p.record_ping(10.0);
            } else {
                p.record_ping_timeout();
            }
        }
        let a = p.availability();
        assert!(a < 0.5 && a > 0.35, "{a}");
    }

    #[test]
    fn rtt_ewma_converges() {
        let mut p = PeerQos::new("peer1");
        for _ in 0..100 {
            p.record_ping(200.0);
        }
        assert!((p.rtt_ewma_ms.unwrap() - 200.0).abs() < 1.0);
    }

    #[test]
    fn the_registry_falls_back_to_unknown_availability() {
        let mut reg = QosRegistry::new();
        reg.entry("p1").record_ping(50.0);
        assert!(reg.availability("p1") > reg.availability("never-seen"));
        assert_eq!(reg.all_availabilities().len(), 1);
    }
}
