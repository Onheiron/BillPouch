use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const RTT_REF_MS: f64 = 500.0;

const ALPHA_RTT: f64 = 0.125;

const BETA_CHALLENGE: f64 = 0.10;

const W_LATENCY: f64 = 0.6;

const W_RELIABILITY: f64 = 0.4;

pub const FAULT_DEGRADED: u8 = 70;

pub const FAULT_SUSPECTED: u8 = 90;

pub const FAULT_BLACKLISTED: u8 = 100;

const FAULT_INCREMENT: u8 = 5;

const FAULT_DECAY: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerQos {
    pub peer_id: String,

    pub rtt_ewma_ms: Option<f64>,

    pub challenge_success_ewma: f64,

    pub last_ping_at: Option<u64>,

    pub challenges_sent: u64,

    pub challenges_succeeded: u64,

    pub fault_score: u8,
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
            fault_score: 0,
        }
    }

    pub fn record_ping(&mut self, rtt_ms: f64) {
        self.rtt_ewma_ms = Some(match self.rtt_ewma_ms {
            None => rtt_ms,
            Some(prev) => ALPHA_RTT * rtt_ms + (1.0 - ALPHA_RTT) * prev,
        });
        self.last_ping_at = Some(chrono::Utc::now().timestamp() as u64);
    }

    pub fn record_ping_timeout(&mut self) {
        self.record_ping(RTT_REF_MS * 10.0);
    }

    pub fn record_challenge(&mut self, success: bool) {
        let outcome = if success { 1.0_f64 } else { 0.0_f64 };
        self.challenge_success_ewma =
            BETA_CHALLENGE * outcome + (1.0 - BETA_CHALLENGE) * self.challenge_success_ewma;
        self.challenges_sent += 1;
        if success {
            self.challenges_succeeded += 1;
        }
    }

    pub fn record_pos_success(&mut self) {
        self.fault_score = self.fault_score.saturating_sub(FAULT_DECAY);
        self.record_challenge(true);
    }

    pub fn record_pos_failure(&mut self) {
        self.fault_score = self.fault_score.saturating_add(FAULT_INCREMENT).min(100);
        self.record_challenge(false);
    }

    pub fn latency_score(&self) -> f64 {
        self.rtt_ewma_ms
            .map_or(0.0, |rtt| (-rtt / RTT_REF_MS).exp())
    }

    pub fn stability_score(&self) -> f64 {
        W_LATENCY * self.latency_score() + W_RELIABILITY * self.challenge_success_ewma
    }

    pub fn is_observed(&self) -> bool {
        self.rtt_ewma_ms.is_some()
    }
}

#[derive(Debug, Default)]
pub struct QosRegistry {
    peers: HashMap<String, PeerQos>,
}

impl QosRegistry {
    pub fn new() -> Self {
        Self::default()
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

    pub fn stability_score(&self, peer_id: &str) -> f64 {
        self.peers
            .get(peer_id)
            .map_or(0.0, PeerQos::stability_score)
    }

    pub fn all_stability_scores(&self) -> Vec<f64> {
        self.peers.values().map(PeerQos::stability_score).collect()
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn peer_ids(&self) -> impl Iterator<Item = &str> {
        self.peers.keys().map(String::as_str)
    }

    pub fn fault_score(&self, peer_id: &str) -> u8 {
        self.peers.get(peer_id).map_or(0, |p| p.fault_score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_peer_has_zero_latency_score() {
        let p = PeerQos::new("peer1");
        assert_eq!(p.latency_score(), 0.0);
        assert_eq!(p.challenge_success_ewma, 1.0);
    }

    #[test]
    fn stability_score_perfect_peer() {
        let mut p = PeerQos::new("peer1");
        p.record_ping(0.0);
        assert!((p.latency_score() - 1.0).abs() < 1e-9);
        assert!((p.stability_score() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn stability_score_at_rtt_ref() {
        let mut p = PeerQos::new("peer1");
        p.rtt_ewma_ms = Some(RTT_REF_MS);
        let expected_lat = (-1.0_f64).exp();
        assert!((p.latency_score() - expected_lat).abs() < 1e-9);
    }

    #[test]
    fn challenge_failure_degrades_score() {
        let mut p = PeerQos::new("peer1");
        p.record_ping(10.0);
        let score_before = p.stability_score();
        for _ in 0..20 {
            p.record_challenge(false);
        }
        assert!(p.stability_score() < score_before);
    }

    #[test]
    fn ping_timeout_degrades_latency() {
        let mut p = PeerQos::new("peer1");
        p.record_ping(10.0);
        let lat_before = p.latency_score();
        for _ in 0..20 {
            p.record_ping_timeout();
        }
        assert!(p.latency_score() < lat_before);
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
    fn registry_entry_creates_new_peer() {
        let mut reg = QosRegistry::new();
        reg.entry("p1").record_ping(50.0);
        assert!(reg.get("p1").is_some());
        assert_eq!(reg.peer_count(), 1);
    }

    #[test]
    fn all_stability_scores_length() {
        let mut reg = QosRegistry::new();
        reg.entry("p1").record_ping(50.0);
        reg.entry("p2").record_ping(100.0);
        reg.entry("p3").record_ping(300.0);
        assert_eq!(reg.all_stability_scores().len(), 3);
    }
}
