use crate::network::qos::FAULT_BLACKLISTED;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const PASS_INC: i64 = 2;
pub const FAIL_DEC: i64 = 10;

const DAY: u64 = 86_400;
const R0_LOCK_DAYS: u64 = 30;

const PROMOTIONS: [(ReputationTier, u64, f64); 3] = [
    (ReputationTier::R4, 365, 0.999),
    (ReputationTier::R3, 90, 0.995),
    (ReputationTier::R2, 30, 0.98),
];

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
    pub fn name(self) -> &'static str {
        match self {
            Self::R0 => "Quarantine",
            Self::R1 => "Fledgling",
            Self::R2 => "Reliable",
            Self::R3 => "Trusted",
            Self::R4 => "Pillar",
        }
    }

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationRecord {
    pub peer_id: String,
    pub reputation_score: i64,
    pub tier: ReputationTier,
    pub first_seen_at: u64,
    pub r0_lock_until: Option<u64>,
    pub pos_challenges_sent: u64,
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

    pub fn evict_without_notice(&mut self, now_secs: u64) {
        self.reputation_score = 0;
        self.tier = ReputationTier::R0;
        self.r0_lock_until = Some(now_secs + R0_LOCK_DAYS * DAY);
    }

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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReputationStore {
    records: HashMap<String, ReputationRecord>,
}

impl ReputationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_create(&mut self, peer_id: &str, now_secs: u64) -> &mut ReputationRecord {
        self.records
            .entry(peer_id.to_string())
            .or_insert_with(|| ReputationRecord::new(peer_id, now_secs))
    }

    pub fn get(&self, peer_id: &str) -> Option<&ReputationRecord> {
        self.records.get(peer_id)
    }

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

        let mut r = veteran(400, 100, 90);
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
        assert_eq!(r.reputation_score, 0);
        assert_eq!((r.pos_challenges_sent, r.pos_challenges_passed), (2, 1));
    }
}
