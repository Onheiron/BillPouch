//! Peer reputation — R0..R4 tiers derived from Proof-of-Storage history.
//!
//! | Tier | Name       | Criteria                                                  |
//! |------|------------|-----------------------------------------------------------|
//! | R0   | Quarantine | `fault_score ≥ FAULT_BLACKLISTED`, or evicted (30-day lock) |
//! | R1   | Fledgling  | Default; new or not yet meeting R2                         |
//! | R2   | Reliable   | Known ≥ 30 d, PoS pass rate ≥ 98 %                         |
//! | R3   | Trusted    | Known ≥ 90 d, PoS pass rate ≥ 99.5 %                       |
//! | R4   | Pillar     | Known ≥ 365 d, PoS pass rate ≥ 99.9 %                      |
//!
//! Records are updated by the quality monitor after every PoS challenge and
//! live in memory for the daemon's lifetime.  `reputation_score` is a
//! running counter (+[`PASS_INC`] / −[`FAIL_DEC`]) shown by `bp status`.

use crate::network::qos::FAULT_BLACKLISTED;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Score increment per passed PoS challenge.
pub const PASS_INC: i64 = 2;
/// Score decrement per failed PoS challenge.
pub const FAIL_DEC: i64 = 10;

const DAY: u64 = 86_400;
const R0_LOCK_DAYS: u64 = 30;

/// `(tier, minimum days known, minimum PoS pass rate)`, best tier first.
const PROMOTIONS: [(ReputationTier, u64, f64); 3] = [
    (ReputationTier::R4, 365, 0.999),
    (ReputationTier::R3, 90, 0.995),
    (ReputationTier::R2, 30, 0.98),
];

/// Discrete reputation tier for a Pouch peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ReputationTier {
    R0,
    R1,
    R2,
    R3,
    R4,
}

impl ReputationTier {
    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            Self::R0 => "Quarantine",
            Self::R1 => "Fledgling",
            Self::R2 => "Reliable",
            Self::R3 => "Trusted",
            Self::R4 => "Pillar",
        }
    }

    /// Target recovery probability `Ph` used to compute the effective
    /// available storage of a Pouch with this tier
    /// (see [`crate::coding::params::compute_network_storage_factor`]).
    pub fn qos_target_ph(self) -> f64 {
        match self {
            Self::R0 => 0.0,
            Self::R1 => 0.70,
            Self::R2 => 0.85,
            Self::R3 => 0.95,
            Self::R4 => 0.999,
        }
    }
}

impl std::fmt::Display for ReputationTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Per-peer reputation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationRecord {
    /// libp2p PeerId (base58).
    pub peer_id: String,
    /// Running score: +PASS_INC per passed challenge, −FAIL_DEC per failure.
    pub reputation_score: i64,
    /// Current tier.
    pub tier: ReputationTier,
    /// Unix timestamp (s) when this peer was first seen.
    pub first_seen_at: u64,
    /// If set, the peer is locked in R0 until this Unix timestamp.
    pub r0_lock_until: Option<u64>,
    /// Total PoS challenges sent to this peer.
    pub pos_challenges_sent: u64,
    /// PoS challenges answered successfully.
    pub pos_challenges_passed: u64,
}

impl ReputationRecord {
    pub fn new(peer_id: impl Into<String>, now_secs: u64) -> Self {
        Self {
            peer_id: peer_id.into(),
            reputation_score: 0,
            tier: ReputationTier::R1,
            first_seen_at: now_secs,
            r0_lock_until: None,
            pos_challenges_sent: 0,
            pos_challenges_passed: 0,
        }
    }

    /// Record a PoS challenge outcome and recompute the tier.
    pub fn record_pos_challenge(&mut self, passed: bool, fault_score: u8, now_secs: u64) {
        self.pos_challenges_sent += 1;
        if passed {
            self.pos_challenges_passed += 1;
            self.reputation_score += PASS_INC;
        } else {
            self.reputation_score = (self.reputation_score - FAIL_DEC).max(0);
        }
        self.recompute_tier(fault_score, now_secs);
    }

    /// Eviction without notice: score reset and R0 lock for 30 days.
    pub fn evict_without_notice(&mut self, now_secs: u64) {
        self.reputation_score = 0;
        self.tier = ReputationTier::R0;
        self.r0_lock_until = Some(now_secs + R0_LOCK_DAYS * DAY);
    }

    /// Recompute the tier from the current counters.
    pub fn recompute_tier(&mut self, fault_score: u8, now_secs: u64) {
        if let Some(lock_until) = self.r0_lock_until {
            if now_secs < lock_until {
                self.tier = ReputationTier::R0;
                return;
            }
            self.r0_lock_until = None;
        }
        if fault_score >= FAULT_BLACKLISTED {
            self.tier = ReputationTier::R0;
            return;
        }
        let age_days = now_secs.saturating_sub(self.first_seen_at) / DAY;
        let pass_rate = if self.pos_challenges_sent == 0 {
            1.0
        } else {
            self.pos_challenges_passed as f64 / self.pos_challenges_sent as f64
        };
        self.tier = PROMOTIONS
            .iter()
            .find(|(_, days, rate)| age_days >= *days && pass_rate >= *rate)
            .map_or(ReputationTier::R1, |(tier, _, _)| *tier);
    }
}

/// In-memory registry of per-peer [`ReputationRecord`]s.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReputationStore {
    records: HashMap<String, ReputationRecord>,
}

impl ReputationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return or create the record for `peer_id`.
    pub fn get_or_create(&mut self, peer_id: &str, now_secs: u64) -> &mut ReputationRecord {
        self.records
            .entry(peer_id.to_string())
            .or_insert_with(|| ReputationRecord::new(peer_id, now_secs))
    }

    pub fn get(&self, peer_id: &str) -> Option<&ReputationRecord> {
        self.records.get(peer_id)
    }

    /// Current tier for `peer_id` (R1 for unknown peers).
    pub fn tier(&self, peer_id: &str) -> ReputationTier {
        self.get(peer_id).map_or(ReputationTier::R1, |r| r.tier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_710_000_000;

    fn veteran(days: u64, sent: u64, passed: u64) -> ReputationRecord {
        let mut r = ReputationRecord::new("peer", NOW - days * DAY);
        r.pos_challenges_sent = sent;
        r.pos_challenges_passed = passed;
        r
    }

    #[test]
    fn new_peer_is_fledgling() {
        assert_eq!(ReputationRecord::new("p", NOW).tier, ReputationTier::R1);
        assert_eq!(ReputationStore::new().tier("unknown"), ReputationTier::R1);
    }

    #[test]
    fn promotion_by_age_and_pass_rate() {
        let mut r = veteran(31, 100, 99);
        r.recompute_tier(0, NOW);
        assert_eq!(r.tier, ReputationTier::R2);

        let mut r = veteran(400, 10_000, 9_995);
        r.recompute_tier(0, NOW);
        assert_eq!(r.tier, ReputationTier::R4);

        let mut r = veteran(400, 100, 90); // too many failures
        r.recompute_tier(0, NOW);
        assert_eq!(r.tier, ReputationTier::R1);
    }

    #[test]
    fn blacklisted_fault_score_forces_r0() {
        let mut r = veteran(31, 100, 99);
        r.recompute_tier(FAULT_BLACKLISTED, NOW);
        assert_eq!(r.tier, ReputationTier::R0);
    }

    #[test]
    fn eviction_locks_r0_for_30_days() {
        let mut r = veteran(100, 100, 100);
        r.evict_without_notice(NOW);
        assert_eq!(r.tier, ReputationTier::R0);
        assert_eq!(r.reputation_score, 0);
        r.recompute_tier(0, NOW + 29 * DAY);
        assert_eq!(r.tier, ReputationTier::R0);
        r.recompute_tier(0, NOW + 31 * DAY);
        assert_ne!(r.tier, ReputationTier::R0);
    }

    #[test]
    fn pos_outcome_updates_score() {
        let mut r = ReputationRecord::new("p", NOW);
        r.record_pos_challenge(true, 0, NOW);
        assert_eq!(r.reputation_score, PASS_INC);
        r.record_pos_challenge(false, 0, NOW);
        assert_eq!(r.reputation_score, 0); // clamped at zero
        assert_eq!((r.pos_challenges_sent, r.pos_challenges_passed), (2, 1));
    }
}
