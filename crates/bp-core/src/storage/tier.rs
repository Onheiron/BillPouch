//! Storage tier system for BillPouch Pouch services.
//!
//! Instead of arbitrary byte quotas, Pouches declare one of five fixed tiers.
//! Each tier defines:
//! - An exact on-disk quota that the Pouch reserves.
//! - Which tier-buckets the Pouch participates in (a T3 Pouch participates in
//!   T1, T2, and T3 fragment placement).
//!
//! ## Tiers
//!
//! | Tier | Name     | Quota  |
//! |------|----------|--------|
//! | T1   | Pebble   | 10 GB  |
//! | T2   | Stone    | 100 GB |
//! | T3   | Boulder  | 500 GB |
//! | T4   | Rock     | 1 TB   |
//! | T5   | Monolith | 5 TB   |
//!
//! ## One Pouch per node per network
//!
//! A daemon rejects a second `hatch pouch --network X` if a Pouch is already
//! active on that network for this identity.  Two correlated Pouches on the
//! same machine defeat the purpose of distributed redundancy.

use crate::error::{BpError, BpResult};
use serde::{Deserialize, Serialize};
use std::fmt;

// ── Tier constants ────────────────────────────────────────────────────────────

/// 10 GiB in bytes.
pub const TIER_T1_BYTES: u64 = 10 * 1024 * 1024 * 1024;
/// 100 GiB in bytes.
pub const TIER_T2_BYTES: u64 = 100 * 1024 * 1024 * 1024;
/// 500 GiB in bytes.
pub const TIER_T3_BYTES: u64 = 500 * 1024 * 1024 * 1024;
/// 1 TiB in bytes.
pub const TIER_T4_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
/// 5 TiB in bytes.
pub const TIER_T5_BYTES: u64 = 5 * 1024 * 1024 * 1024 * 1024;

// ── StorageTier enum ──────────────────────────────────────────────────────────

/// A fixed storage tier for a Pouch service.
///
/// Tiers are ordered: `T1 < T2 < T3 < T4 < T5`.  A Pouch at tier T participates
/// in fragment placement for all tiers ≤ T (e.g. a T3 Pouch stores fragments
/// from T1, T2, and T3 files).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum StorageTier {
    /// 10 GiB — "Pebble"
    T1,
    /// 100 GiB — "Stone"
    T2,
    /// 500 GiB — "Boulder"
    T3,
    /// 1 TiB — "Rock"
    T4,
    /// 5 TiB — "Monolith"
    T5,
}

impl StorageTier {
    /// Return the exact quota in bytes for this tier.
    pub fn quota_bytes(self) -> u64 {
        match self {
            StorageTier::T1 => TIER_T1_BYTES,
            StorageTier::T2 => TIER_T2_BYTES,
            StorageTier::T3 => TIER_T3_BYTES,
            StorageTier::T4 => TIER_T4_BYTES,
            StorageTier::T5 => TIER_T5_BYTES,
        }
    }

    /// Human-readable name for this tier.
    pub fn name(self) -> &'static str {
        match self {
            StorageTier::T1 => "Pebble",
            StorageTier::T2 => "Stone",
            StorageTier::T3 => "Boulder",
            StorageTier::T4 => "Rock",
            StorageTier::T5 => "Monolith",
        }
    }

    /// Return all tiers from T1 up to and including `self`.
    ///
    /// Used to determine which tier-buckets a Pouch participates in.
    pub fn participating_tiers(self) -> &'static [StorageTier] {
        match self {
            StorageTier::T1 => &[StorageTier::T1],
            StorageTier::T2 => &[StorageTier::T1, StorageTier::T2],
            StorageTier::T3 => &[StorageTier::T1, StorageTier::T2, StorageTier::T3],
            StorageTier::T4 => &[
                StorageTier::T1,
                StorageTier::T2,
                StorageTier::T3,
                StorageTier::T4,
            ],
            StorageTier::T5 => &[
                StorageTier::T1,
                StorageTier::T2,
                StorageTier::T3,
                StorageTier::T4,
                StorageTier::T5,
            ],
        }
    }

    /// Return all defined tiers in ascending order.
    pub fn all() -> &'static [StorageTier] {
        &[
            StorageTier::T1,
            StorageTier::T2,
            StorageTier::T3,
            StorageTier::T4,
            StorageTier::T5,
        ]
    }

    /// Parse a tier from a string like `"T1"`, `"t2"`, `"T3"`, etc.
    pub fn parse(s: &str) -> BpResult<Self> {
        match s.to_uppercase().as_str() {
            "T1" => Ok(StorageTier::T1),
            "T2" => Ok(StorageTier::T2),
            "T3" => Ok(StorageTier::T3),
            "T4" => Ok(StorageTier::T4),
            "T5" => Ok(StorageTier::T5),
            other => Err(BpError::InvalidInput(format!(
                "unknown storage tier '{}'; valid values are T1, T2, T3, T4, T5",
                other
            ))),
        }
    }

    /// Return the minimum tier that can hold a file of the given byte size.
    ///
    /// Returns `None` if the file is too large even for T5.
    pub fn for_file_size(bytes: u64) -> Option<Self> {
        if bytes <= TIER_T1_BYTES {
            Some(StorageTier::T1)
        } else if bytes <= TIER_T2_BYTES {
            Some(StorageTier::T2)
        } else if bytes <= TIER_T3_BYTES {
            Some(StorageTier::T3)
        } else if bytes <= TIER_T4_BYTES {
            Some(StorageTier::T4)
        } else if bytes <= TIER_T5_BYTES {
            Some(StorageTier::T5)
        } else {
            None
        }
    }
}

impl fmt::Display for StorageTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (label, name) = match self {
            StorageTier::T1 => ("T1", "Pebble  — 10 GB"),
            StorageTier::T2 => ("T2", "Stone   — 100 GB"),
            StorageTier::T3 => ("T3", "Boulder — 500 GB"),
            StorageTier::T4 => ("T4", "Rock    — 1 TB"),
            StorageTier::T5 => ("T5", "Monolith — 5 TB"),
        };
        write!(f, "{} ({})", label, name)
    }
}

impl std::str::FromStr for StorageTier {
    type Err = BpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        StorageTier::parse(s)
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_quota_bytes_correct() {
        assert_eq!(StorageTier::T1.quota_bytes(), 10 * 1024 * 1024 * 1024);
        assert_eq!(StorageTier::T2.quota_bytes(), 100 * 1024 * 1024 * 1024);
        assert_eq!(StorageTier::T3.quota_bytes(), 500 * 1024 * 1024 * 1024);
        assert_eq!(StorageTier::T4.quota_bytes(), 1024 * 1024 * 1024 * 1024);
        assert_eq!(StorageTier::T5.quota_bytes(), 5 * 1024 * 1024 * 1024 * 1024);
    }

    #[test]
    fn tier_ordering() {
        assert!(StorageTier::T1 < StorageTier::T2);
        assert!(StorageTier::T2 < StorageTier::T3);
        assert!(StorageTier::T3 < StorageTier::T4);
        assert!(StorageTier::T4 < StorageTier::T5);
    }

    #[test]
    fn participating_tiers_correct() {
        assert_eq!(StorageTier::T1.participating_tiers(), &[StorageTier::T1]);
        assert_eq!(
            StorageTier::T3.participating_tiers(),
            &[StorageTier::T1, StorageTier::T2, StorageTier::T3]
        );
        assert_eq!(StorageTier::T5.participating_tiers().len(), 5);
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!(StorageTier::parse("t1").unwrap(), StorageTier::T1);
        assert_eq!(StorageTier::parse("T3").unwrap(), StorageTier::T3);
        assert_eq!(StorageTier::parse("T5").unwrap(), StorageTier::T5);
        assert!(StorageTier::parse("T6").is_err());
        assert!(StorageTier::parse("pebble").is_err());
    }

    #[test]
    fn for_file_size() {
        assert_eq!(StorageTier::for_file_size(1024), Some(StorageTier::T1));
        assert_eq!(
            StorageTier::for_file_size(TIER_T1_BYTES),
            Some(StorageTier::T1)
        );
        assert_eq!(
            StorageTier::for_file_size(TIER_T1_BYTES + 1),
            Some(StorageTier::T2)
        );
        assert_eq!(
            StorageTier::for_file_size(TIER_T5_BYTES),
            Some(StorageTier::T5)
        );
        assert_eq!(StorageTier::for_file_size(TIER_T5_BYTES + 1), None);
    }

    #[test]
    fn serde_roundtrip() {
        let tier = StorageTier::T3;
        let json = serde_json::to_string(&tier).unwrap();
        assert_eq!(json, "\"T3\"");
        let back: StorageTier = serde_json::from_str(&json).unwrap();
        assert_eq!(back, StorageTier::T3);
    }

    #[test]
    fn from_str_trait() {
        use std::str::FromStr;
        assert_eq!(StorageTier::from_str("T2").unwrap(), StorageTier::T2);
    }
}
