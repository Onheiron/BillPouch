//! Peer reputation system — R0..R4 tiers computed from QoS history.
//!
//! ## Tiers
//!
//! | Tier | Name       | Criteria                                                         |
//! |------|------------|------------------------------------------------------------------|
//! | R0   | Quarantine | fault_score ≥ FAULT_BLACKLISTED; isolated from trusted peers     |
//! | R1   | Fledgling  | Node age < FLEDGLING_DAYS (7 days in the network)               |
//! | R2   | Reliable   | uptime_ratio ≥ 0.95 over 30 d; pos_pass_rate ≥ 0.98             |
//! | R3   | Trusted    | uptime_ratio ≥ 0.99 over 90 d; pos_pass_rate ≥ 0.995            |
//! | R4   | Pillar     | uptime_ratio ≥ 0.999 over 365 d; pos_pass_rate ≥ 0.999          |
//!
//! ## Reputation score
//!
//! A continuous `reputation_score: i64` underlies the tier computation.
//! Score changes:
//!
//! | Event                         | Delta         |
//! |-------------------------------|---------------|
//! | PoS challenge passed          | +PASS_INC     |
//! | PoS challenge failed          | −FAIL_DEC     |
//! | 24 h verified uptime          | +UPTIME_INC   |
//! | `bp pause --eta` respected    | 0             |
//! | `bp pause --eta` overrun      | −LATE_PENALTY |
//! | Eviction without notice       | score reset→0, R0 lock for 30 days |
//!
//! ## Placement preference
//!
//! When selecting Pouch peers for fragment distribution (`PutFile`), the
//! scheduler **prefers** peers whose reputation tier is ≥ the sender's
//! own tier. R0 peers only receive fragments from other R0 senders.

use crate::network::qos::FAULT_BLACKLISTED;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Days since first-seen after which a node graduates from Fledgling (R1).
pub const FLEDGLING_DAYS: u64 = 7;

/// Minimum uptime ratio (0–1) required for R2.
pub const R2_UPTIME: f64 = 0.95;
/// Minimum PoS pass rate required for R2.
pub const R2_POS_RATE: f64 = 0.98;
/// Observation window for R2 criteria (days).
pub const R2_WINDOW_DAYS: u64 = 30;

/// Minimum uptime ratio required for R3.
pub const R3_UPTIME: f64 = 0.99;
/// Minimum PoS pass rate required for R3.
pub const R3_POS_RATE: f64 = 0.995;
/// Observation window for R3 criteria (days).
pub const R3_WINDOW_DAYS: u64 = 90;

/// Minimum uptime ratio required for R4.
pub const R4_UPTIME: f64 = 0.999;
/// Minimum PoS pass rate required for R4.
pub const R4_POS_RATE: f64 = 0.999;
/// Observation window for R4 criteria (days).
pub const R4_WINDOW_DAYS: u64 = 365;

/// Score increment per passed PoS challenge.
pub const PASS_INC: i64 = 2;
/// Score decrement per failed PoS challenge.
pub const FAIL_DEC: i64 = 10;
/// Score increment per 24 h of verified uptime.
pub const UPTIME_INC: i64 = 1;
/// Penalty for overrunning a `bp pause --eta` deadline.
pub const LATE_PENALTY: i64 = 5;

// ── ReputationTier ────────────────────────────────────────────────────────────

/// Discrete reputation tier for a Pouch peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ReputationTier {
    /// Quarantined — isolated from trusted peers, may be evicted.
    R0,
    /// Fledgling — new node, < 7 days in the network.
    R1,
    /// Reliable — sustained uptime and PoS pass rate over 30 days.
    R2,
    /// Trusted — high reliability over 90 days.
    R3,
    /// Pillar — exemplary reliability over 365 days.
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

    /// Weight `[0.0, 1.0]` applied to `(bid − used)` to obtain the
    /// **effective available bytes** that the network can trust from a
    /// node at this reputation tier.
    ///
    /// | Tier | Factor | Rationale                                  |
    /// |------|--------|--------------------------------------------|
    /// | R0   | 0.00   | Quarantined — no capacity trusted          |
    /// | R1   | 0.10   | Fledgling — unproven, grant 10 %           |
    /// | R2   | 0.70   | Reliable  — sustained track record        |
    /// | R3   | 0.90   | Trusted   — high long-term reliability     |
    /// | R4   | 1.00   | Pillar    — full trust                     |
    pub fn availability_factor(self) -> f64 {
        match self {
            Self::R0 => 0.00,
            Self::R1 => 0.10,
            Self::R2 => 0.70,
            Self::R3 => 0.90,
            Self::R4 => 1.00,
        }
    }

    /// Returns `true` if this tier is trusted enough to receive fragments
    /// from senders with `sender_tier`.
    ///
    /// Rule: a Pouch is eligible for a sender's fragments if
    /// `pouch_tier >= sender_tier || sender_tier == R0`.
    pub fn is_eligible_for(self, sender_tier: ReputationTier) -> bool {
        if sender_tier == ReputationTier::R0 {
            self == ReputationTier::R0
        } else {
            self >= sender_tier
        }
    }
}

impl std::fmt::Display for ReputationTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ── ReputationRecord ──────────────────────────────────────────────────────────

/// Per-peer reputation state persisted for the daemon's lifetime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationRecord {
    /// libp2p PeerId (base58).
    pub peer_id: String,

    /// Continuous score used for tier promotion / demotion.
    pub reputation_score: i64,

    /// Current computed tier.
    pub tier: ReputationTier,

    /// Unix timestamp (seconds) when this peer was first seen.
    pub first_seen_at: u64,

    /// Unix timestamp of the last score update.
    pub last_updated_at: u64,

    /// If set, the peer is R0-locked until this Unix timestamp.
    pub r0_lock_until: Option<u64>,

    /// Cumulative uptime seconds observed.
    pub uptime_seconds: u64,

    /// Total PoS challenges sent to this peer.
    pub pos_challenges_sent: u64,

    /// PoS challenges that this peer answered successfully.
    pub pos_challenges_passed: u64,
}

impl ReputationRecord {
    /// Create a brand-new reputation record for a newly discovered peer.
    pub fn new(peer_id: impl Into<String>, now_secs: u64) -> Self {
        Self {
            peer_id: peer_id.into(),
            reputation_score: 0,
            tier: ReputationTier::R1,
            first_seen_at: now_secs,
            last_updated_at: now_secs,
            r0_lock_until: None,
            uptime_seconds: 0,
            pos_challenges_sent: 0,
            pos_challenges_passed: 0,
        }
    }

    /// PoS challenge outcome — update score and challenge counters.
    pub fn record_pos_challenge(&mut self, passed: bool, fault_score: u8, now_secs: u64) {
        self.pos_challenges_sent += 1;
        if passed {
            self.pos_challenges_passed += 1;
            self.reputation_score += PASS_INC;
        } else {
            self.reputation_score = (self.reputation_score - FAIL_DEC).max(0);
        }
        self.last_updated_at = now_secs;
        self.recompute_tier(fault_score, now_secs);
    }

    /// Credit verified uptime.  Call once per continuous 24 h window.
    pub fn credit_uptime_day(&mut self, fault_score: u8, now_secs: u64) {
        self.uptime_seconds += 86_400;
        self.reputation_score += UPTIME_INC;
        self.last_updated_at = now_secs;
        self.recompute_tier(fault_score, now_secs);
    }

    /// Apply the late-pause penalty (pause ETA overrun).
    pub fn penalise_late_pause(&mut self, fault_score: u8, now_secs: u64) {
        self.reputation_score = (self.reputation_score - LATE_PENALTY).max(0);
        self.last_updated_at = now_secs;
        self.recompute_tier(fault_score, now_secs);
    }

    /// Forced eviction without notice — reset score and lock into R0 for 30 days.
    pub fn evict_without_notice(&mut self, now_secs: u64) {
        self.reputation_score = 0;
        self.tier = ReputationTier::R0;
        self.r0_lock_until = Some(now_secs + 30 * 86_400);
        self.last_updated_at = now_secs;
    }

    /// Recompute the tier from current stats.
    ///
    /// Call after every mutation that changes score, uptime, or PoS counters.
    pub fn recompute_tier(&mut self, fault_score: u8, now_secs: u64) {
        // R0 lock still active?
        if let Some(lock_until) = self.r0_lock_until {
            if now_secs < lock_until {
                self.tier = ReputationTier::R0;
                return;
            }
            // Lock expired — clear it and re-evaluate
            self.r0_lock_until = None;
        }

        // Quarantine: fault_score maxed out
        if fault_score >= FAULT_BLACKLISTED {
            self.tier = ReputationTier::R0;
            return;
        }

        let age_days = (now_secs.saturating_sub(self.first_seen_at)) / 86_400;
        let pos_rate = if self.pos_challenges_sent == 0 {
            1.0_f64
        } else {
            self.pos_challenges_passed as f64 / self.pos_challenges_sent as f64
        };
        // Uptime ratio is computed over the node's full age.
        let uptime_ratio = if age_days == 0 {
            1.0_f64
        } else {
            self.uptime_seconds as f64 / (age_days as f64 * 86_400.0)
        };

        self.tier = if age_days >= R4_WINDOW_DAYS
            && uptime_ratio >= R4_UPTIME
            && pos_rate >= R4_POS_RATE
        {
            ReputationTier::R4
        } else if age_days >= R3_WINDOW_DAYS && uptime_ratio >= R3_UPTIME && pos_rate >= R3_POS_RATE
        {
            ReputationTier::R3
        } else if age_days >= R2_WINDOW_DAYS && uptime_ratio >= R2_UPTIME && pos_rate >= R2_POS_RATE
        {
            ReputationTier::R2
        } else if age_days < FLEDGLING_DAYS {
            ReputationTier::R1
        } else {
            // Past fledgling window but not yet meeting R2 criteria
            ReputationTier::R1
        };
    }

    /// PoS pass rate in `[0.0, 1.0]`.
    pub fn pos_pass_rate(&self) -> f64 {
        if self.pos_challenges_sent == 0 {
            1.0
        } else {
            self.pos_challenges_passed as f64 / self.pos_challenges_sent as f64
        }
    }
}

// ── ReputationStore ───────────────────────────────────────────────────────────

/// In-memory registry of per-peer [`ReputationRecord`]s.
///
/// Stored in `DaemonState` behind an `RwLock`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReputationStore {
    records: HashMap<String, ReputationRecord>,
}

impl ReputationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return or create a record for `peer_id`.
    pub fn get_or_create(&mut self, peer_id: &str, now_secs: u64) -> &mut ReputationRecord {
        self.records
            .entry(peer_id.to_string())
            .or_insert_with(|| ReputationRecord::new(peer_id, now_secs))
    }

    /// Look up an existing record (read-only).
    pub fn get(&self, peer_id: &str) -> Option<&ReputationRecord> {
        self.records.get(peer_id)
    }

    /// Current tier for `peer_id` (default: R1 for unknown peers).
    pub fn tier(&self, peer_id: &str) -> ReputationTier {
        self.records
            .get(peer_id)
            .map(|r| r.tier)
            .unwrap_or(ReputationTier::R1)
    }

    /// All records, for inspection / gossip.
    pub fn all(&self) -> impl Iterator<Item = &ReputationRecord> {
        self.records.values()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_710_000_000; // arbitrary fixed timestamp

    #[test]
    fn new_peer_is_fledgling() {
        let r = ReputationRecord::new("peer1", NOW);
        assert_eq!(r.tier, ReputationTier::R1);
    }

    #[test]
    fn blacklisted_fault_score_forces_r0() {
        let mut r = ReputationRecord::new("peer1", NOW);
        // Simulate many uptime days so it would otherwise be R2+
        r.uptime_seconds = 31 * 86_400;
        r.first_seen_at = NOW - 31 * 86_400;
        r.pos_challenges_sent = 100;
        r.pos_challenges_passed = 99;
        r.recompute_tier(100 /* FAULT_BLACKLISTED */, NOW);
        assert_eq!(r.tier, ReputationTier::R0);
    }

    #[test]
    fn eviction_sets_r0_lock_30_days() {
        let mut r = ReputationRecord::new("peer1", NOW);
        r.evict_without_notice(NOW);
        assert_eq!(r.tier, ReputationTier::R0);
        assert_eq!(r.r0_lock_until, Some(NOW + 30 * 86_400));
        assert_eq!(r.reputation_score, 0);
    }

    #[test]
    fn r0_lock_expires() {
        let mut r = ReputationRecord::new("peer1", NOW - 40 * 86_400);
        r.evict_without_notice(NOW - 40 * 86_400);
        // Re-evaluate after lock expired and enough uptime
        let later = NOW;
        r.uptime_seconds = 31 * 86_400;
        r.pos_challenges_sent = 100;
        r.pos_challenges_passed = 99;
        r.recompute_tier(0, later);
        // Lock expired (was 30 days, now 40 days later) — should promote
        assert_ne!(r.tier, ReputationTier::R0);
    }

    #[test]
    fn pos_pass_increments_score() {
        let mut r = ReputationRecord::new("peer1", NOW);
        let initial = r.reputation_score;
        r.record_pos_challenge(true, 0, NOW);
        assert_eq!(r.reputation_score, initial + PASS_INC);
    }

    #[test]
    fn pos_fail_decrements_score_clamped_at_zero() {
        let mut r = ReputationRecord::new("peer1", NOW);
        r.reputation_score = 3;
        r.record_pos_challenge(false, 0, NOW);
        assert_eq!(r.reputation_score, 0); // clamped, not negative
    }

    #[test]
    fn is_eligible_for_placement() {
        // R0 pouch only receives R0 sender fragments
        assert!(ReputationTier::R0.is_eligible_for(ReputationTier::R0));
        assert!(!ReputationTier::R0.is_eligible_for(ReputationTier::R1));

        // R2 pouch accepts R1 and R2 senders (>= sender_tier)
        assert!(ReputationTier::R2.is_eligible_for(ReputationTier::R1));
        assert!(ReputationTier::R2.is_eligible_for(ReputationTier::R2));
        assert!(!ReputationTier::R2.is_eligible_for(ReputationTier::R3));

        // R4 accepts everyone (>= R0, R1, ..., R4)
        assert!(ReputationTier::R4.is_eligible_for(ReputationTier::R2));
        assert!(ReputationTier::R4.is_eligible_for(ReputationTier::R4));
    }

    #[test]
    fn store_tier_unknown_peer_defaults_r1() {
        let store = ReputationStore::new();
        assert_eq!(store.tier("unknown_peer"), ReputationTier::R1);
    }

    #[test]
    fn tier_display() {
        assert_eq!(ReputationTier::R0.to_string(), "Quarantine");
        assert_eq!(ReputationTier::R4.to_string(), "Pillar");
    }
}
