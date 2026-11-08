use crate::error::{BpError, BpResult};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const TIER_T1_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub const TIER_T2_BYTES: u64 = 100 * 1024 * 1024 * 1024;
pub const TIER_T3_BYTES: u64 = 500 * 1024 * 1024 * 1024;
pub const TIER_T4_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub const TIER_T5_BYTES: u64 = 5 * 1024 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum StorageTier {
    T1,
    T2,
    T3,
    T4,
    T5,
}

impl StorageTier {
    pub fn quota_bytes(self) -> u64 {
        match self {
            StorageTier::T1 => TIER_T1_BYTES,
            StorageTier::T2 => TIER_T2_BYTES,
            StorageTier::T3 => TIER_T3_BYTES,
            StorageTier::T4 => TIER_T4_BYTES,
            StorageTier::T5 => TIER_T5_BYTES,
        }
    }

    pub fn fitting(bytes: u64) -> Self {
        Self::all()
            .iter()
            .rev()
            .copied()
            .find(|t| t.quota_bytes() <= bytes)
            .unwrap_or(StorageTier::T1)
    }

    pub fn below(self) -> Option<Self> {
        Self::all().iter().copied().rev().find(|t| *t < self)
    }

    pub fn slice_bytes(self) -> u64 {
        self.quota_bytes() - self.below().map_or(0, StorageTier::quota_bytes)
    }

    pub fn layers(self) -> impl Iterator<Item = StorageTier> {
        Self::all().iter().copied().filter(move |t| *t <= self)
    }

    pub fn name(self) -> &'static str {
        match self {
            StorageTier::T1 => "Pebble",
            StorageTier::T2 => "Stone",
            StorageTier::T3 => "Boulder",
            StorageTier::T4 => "Rock",
            StorageTier::T5 => "Monolith",
        }
    }

    pub fn all() -> &'static [StorageTier] {
        &[
            StorageTier::T1,
            StorageTier::T2,
            StorageTier::T3,
            StorageTier::T4,
            StorageTier::T5,
        ]
    }

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
}

impl fmt::Display for StorageTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (label, name) = match self {
            StorageTier::T1 => ("T1", "Pebble — 10 GiB"),
            StorageTier::T2 => ("T2", "Stone — 100 GiB"),
            StorageTier::T3 => ("T3", "Boulder — 500 GiB"),
            StorageTier::T4 => ("T4", "Rock — 1 TiB"),
            StorageTier::T5 => ("T5", "Monolith — 5 TiB"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_are_nested_layers_with_their_own_slice() {
        assert_eq!(StorageTier::T1.slice_bytes(), TIER_T1_BYTES);
        assert_eq!(StorageTier::T2.slice_bytes(), TIER_T2_BYTES - TIER_T1_BYTES);
        let slices: u64 = StorageTier::T3.layers().map(StorageTier::slice_bytes).sum();
        assert_eq!(slices, TIER_T3_BYTES);
        assert_eq!(
            StorageTier::T3.layers().collect::<Vec<_>>(),
            vec![StorageTier::T1, StorageTier::T2, StorageTier::T3]
        );
        assert_eq!(StorageTier::fitting(TIER_T2_BYTES + 1), StorageTier::T2);
        assert_eq!(StorageTier::fitting(1), StorageTier::T1);
        assert_eq!(StorageTier::T1.below(), None);
    }

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
    fn parse_case_insensitive() {
        assert_eq!(StorageTier::parse("t1").unwrap(), StorageTier::T1);
        assert_eq!(StorageTier::parse("T3").unwrap(), StorageTier::T3);
        assert_eq!(StorageTier::parse("T5").unwrap(), StorageTier::T5);
        assert!(StorageTier::parse("T6").is_err());
        assert!(StorageTier::parse("pebble").is_err());
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
