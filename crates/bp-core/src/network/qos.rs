//! Per-peer Quality-of-Service (QoS) tracking.
//!
//! Every known Pouch in the network gets a [`PeerQos`] record that is updated
//! whenever the local node issues a Ping challenge or a Proof-of-Storage
//! challenge and receives (or times out on) a response.
//!
//! ## Stability score
//!
//! The **stability score** `s ∈ [0.0, 1.0]` summarises two independent signals:
//!
//! ```text
//! s(p) = w_lat · latency_score(rtt_ewma) + w_rel · challenge_success_ewma
//!
//! latency_score(rtt_ms) = exp(−rtt_ms / RTT_REF_MS)   (RTT_REF_MS = 500 ms)
//! ```
//!
//! Default weights: `w_lat = 0.6`, `w_rel = 0.4`.
//!
//! ## EWMA smoothing
//!
//! | Signal          | Factor | Rationale                              |
//! |-----------------|--------|----------------------------------------|
//! | RTT             | α=0.125| TCP-standard (RFC 6298)                |
//! | Challenge result| β=0.10 | Slower decay → stable reliability view |
//!
//! A new peer starts with `rtt_ewma_ms = None` (latency component = 0) and
//! `challenge_success_ewma = 1.0` (benefit of the doubt, falls quickly on failure).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Reference RTT in ms: latency_score = 1.0 at 0 ms, 0.37 at RTT_REF_MS.
const RTT_REF_MS: f64 = 500.0;

/// EWMA smoothing factor for RTT (TCP-standard, RFC 6298).
const ALPHA_RTT: f64 = 0.125;

/// EWMA smoothing factor for challenge success rate (slower for stability).
const BETA_CHALLENGE: f64 = 0.10;

/// Weight of the latency component in the composite stability score.
const W_LATENCY: f64 = 0.6;

/// Weight of the reliability component in the composite stability score.
const W_RELIABILITY: f64 = 0.4;

// ── Fault-score thresholds ────────────────────────────────────────────────────

/// Fault score (0–100) at which a peer is considered *degraded* — no new
/// fragments will be assigned to it.
pub const FAULT_DEGRADED: u8 = 70;

/// Fault score at which a peer is *suspected* — preventive recoding begins.
pub const FAULT_SUSPECTED: u8 = 90;

/// Fault score at which a peer is *blacklisted* — evicted from routing and
/// announced as blacklisted on gossip.
pub const FAULT_BLACKLISTED: u8 = 100;

/// Points added to `fault_score` per failed Proof-of-Storage challenge.
const FAULT_INCREMENT: u8 = 5;

/// Points subtracted from `fault_score` per successful PoS challenge.
const FAULT_DECAY: u8 = 1;

// ── PeerQos ───────────────────────────────────────────────────────────────────

/// Live QoS data for a single remote Pouch peer.
///
/// Serialisable so that it can be persisted across daemon restarts or
/// gossipped as part of a `ChallengeResult` announcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerQos {
    /// libp2p PeerId (base58).
    pub peer_id: String,

    /// EWMA round-trip time in milliseconds.  `None` until the first Ping
    /// response is received.
    pub rtt_ewma_ms: Option<f64>,

    /// EWMA of challenge success rate in `[0.0, 1.0]`.
    /// Starts at `1.0` (benefit of the doubt).
    pub challenge_success_ewma: f64,

    /// Unix timestamp (seconds) of the most recent Ping exchange.
    pub last_ping_at: Option<u64>,

    /// Total Ping + PoS challenges sent.
    pub challenges_sent: u64,

    /// Total successful challenge responses received.
    pub challenges_succeeded: u64,

    /// Accumulated fault score in `[0, 100]`.
    ///
    /// Incremented by `FAULT_INCREMENT` on each failed Proof-of-Storage
    /// challenge; decremented by `FAULT_DECAY` on each success.
    /// At [`FAULT_DEGRADED`] (70) no new fragments are sent; at
    /// [`FAULT_SUSPECTED`] (90) preventive recoding begins; at
    /// [`FAULT_BLACKLISTED`] (100) the peer is evicted.
    pub fault_score: u8,
}

impl PeerQos {
    /// Create a fresh QoS record for a newly discovered peer.
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

    // ── Measurements ─────────────────────────────────────────────────────

    /// Incorporate a new RTT sample (milliseconds) into the EWMA.
    ///
    /// Also updates `last_ping_at` to the current UTC time.
    pub fn record_ping(&mut self, rtt_ms: f64) {
        self.rtt_ewma_ms = Some(match self.rtt_ewma_ms {
            None => rtt_ms,
            Some(prev) => ALPHA_RTT * rtt_ms + (1.0 - ALPHA_RTT) * prev,
        });
        self.last_ping_at = Some(chrono::Utc::now().timestamp() as u64);
    }

    /// Record the outcome of a Ping timeout (no response within deadline).
    ///
    /// The latency component decays toward 0 by increasing the EWMA with a
    /// very large RTT value (10 × reference), signalling poor reachability.
    pub fn record_ping_timeout(&mut self) {
        self.record_ping(RTT_REF_MS * 10.0);
    }

    /// Incorporate the result of a challenge (Ping or Proof-of-Storage).
    ///
    /// `success = true`  → response was received and validated.
    /// `success = false` → response timed out or proof was invalid.
    pub fn record_challenge(&mut self, success: bool) {
        let outcome = if success { 1.0_f64 } else { 0.0_f64 };
        self.challenge_success_ewma =
            BETA_CHALLENGE * outcome + (1.0 - BETA_CHALLENGE) * self.challenge_success_ewma;
        self.challenges_sent += 1;
        if success {
            self.challenges_succeeded += 1;
        }
    }

    /// Record a successful Proof-of-Storage response.
    ///
    /// Decays `fault_score` by [`FAULT_DECAY`] and records a successful
    /// challenge in the reliability EWMA.
    pub fn record_pos_success(&mut self) {
        self.fault_score = self.fault_score.saturating_sub(FAULT_DECAY);
        self.record_challenge(true);
    }

    /// Record a failed Proof-of-Storage response (timeout or invalid proof).
    ///
    /// Increments `fault_score` by [`FAULT_INCREMENT`] (capped at 100) and
    /// records a failed challenge in the reliability EWMA.
    pub fn record_pos_failure(&mut self) {
        self.fault_score = self.fault_score.saturating_add(FAULT_INCREMENT).min(100);
        self.record_challenge(false);
    }

    /// Current fault status derived from [`Self::fault_score`].
    ///
    /// Returns `"ok"`, `"degraded"`, `"suspected"`, or `"blacklisted"`.
    pub fn fault_status(&self) -> &'static str {
        match self.fault_score {
            s if s >= FAULT_BLACKLISTED => "blacklisted",
            s if s >= FAULT_SUSPECTED => "suspected",
            s if s >= FAULT_DEGRADED => "degraded",
            _ => "ok",
        }
    }

    // ── Derived scores ────────────────────────────────────────────────────

    /// Latency component of the stability score: `exp(−rtt_ewma / RTT_REF_MS)`.
    ///
    /// Returns `0.0` if no Ping has been recorded yet.
    pub fn latency_score(&self) -> f64 {
        self.rtt_ewma_ms
            .map_or(0.0, |rtt| (-rtt / RTT_REF_MS).exp())
    }

    /// Composite stability score in `[0.0, 1.0]`.
    ///
    /// ```text
    /// s = W_LATENCY · latency_score + W_RELIABILITY · challenge_success_ewma
    /// ```
    ///
    /// This value feeds directly into `coding::params::compute_coding_params` to
    /// determine the recovery threshold `k` for a target high probability `Ph`.
    pub fn stability_score(&self) -> f64 {
        W_LATENCY * self.latency_score() + W_RELIABILITY * self.challenge_success_ewma
    }

    /// Whether the peer has been pinged at all.
    pub fn is_observed(&self) -> bool {
        self.rtt_ewma_ms.is_some()
    }
}

// ── QosRegistry ───────────────────────────────────────────────────────────────

/// In-memory map of `peer_id → PeerQos` for the local node.
///
/// Held inside `DaemonState` under a `RwLock`:
///
/// ```ignore
/// pub qos: RwLock<QosRegistry>,
/// ```
#[derive(Debug, Default)]
pub struct QosRegistry {
    peers: HashMap<String, PeerQos>,
}

impl QosRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return (or lazily create) the QoS record for `peer_id`.
    pub fn entry(&mut self, peer_id: impl Into<String>) -> &mut PeerQos {
        let key = peer_id.into();
        self.peers
            .entry(key.clone())
            .or_insert_with(|| PeerQos::new(key))
    }

    /// Read-only view of a peer's QoS; returns `None` if never seen.
    pub fn get(&self, peer_id: &str) -> Option<&PeerQos> {
        self.peers.get(peer_id)
    }

    /// Remove a peer (e.g. on explicit Farewell or eviction).
    pub fn remove(&mut self, peer_id: &str) {
        self.peers.remove(peer_id);
    }

    /// Stability score for `peer_id`.  Returns `0.0` if never seen.
    pub fn stability_score(&self, peer_id: &str) -> f64 {
        self.peers
            .get(peer_id)
            .map_or(0.0, PeerQos::stability_score)
    }

    /// Snapshot of all stability scores as a `Vec<f64>` (order not defined).
    ///
    /// This is the primary input to [`crate::coding::params::compute_coding_params`].
    pub fn all_stability_scores(&self) -> Vec<f64> {
        self.peers.values().map(PeerQos::stability_score).collect()
    }

    /// Stability scores only for Pouch peers whose `peer_id` appears in
    /// `pouch_peer_ids` — used to scope the k-calculation to active Pouches
    /// in a specific network, rather than all connected peers.
    pub fn stability_scores_for<'a>(
        &self,
        pouch_peer_ids: impl Iterator<Item = &'a str>,
    ) -> Vec<f64> {
        pouch_peer_ids.map(|id| self.stability_score(id)).collect()
    }

    /// Total number of tracked peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// All peer IDs currently tracked.
    pub fn peer_ids(&self) -> impl Iterator<Item = &str> {
        self.peers.keys().map(String::as_str)
    }

    /// Fault status of a peer: `"ok"`, `"degraded"`, `"suspected"`, or
    /// `"blacklisted"`.  Returns `"ok"` if the peer has never been seen.
    pub fn fault_status(&self, peer_id: &str) -> &'static str {
        self.peers
            .get(peer_id)
            .map_or("ok", PeerQos::fault_status)
    }

    /// Fault score of a peer in `[0, 100]`.  Returns `0` if never seen.
    pub fn fault_score(&self, peer_id: &str) -> u8 {
        self.peers.get(peer_id).map_or(0, |p| p.fault_score)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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
        // 0 ms RTT → latency_score = 1.0
        p.record_ping(0.0);
        assert!((p.latency_score() - 1.0).abs() < 1e-9);
        // stability = 0.6*1.0 + 0.4*1.0 = 1.0
        assert!((p.stability_score() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn stability_score_at_rtt_ref() {
        let mut p = PeerQos::new("peer1");
        // After many samples of RTT_REF_MS the EWMA converges to RTT_REF_MS.
        // Use a single perfect-convergence shortcut:
        p.rtt_ewma_ms = Some(RTT_REF_MS);
        // latency_score = exp(-1) ≈ 0.3679
        let expected_lat = (-1.0_f64).exp();
        assert!((p.latency_score() - expected_lat).abs() < 1e-9);
    }

    #[test]
    fn challenge_failure_degrades_score() {
        let mut p = PeerQos::new("peer1");
        p.record_ping(10.0); // fast peer
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
        // Feed many identical samples: EWMA should converge to that value.
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
