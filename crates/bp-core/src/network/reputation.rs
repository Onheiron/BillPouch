use serde::{Deserialize, Serialize};

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
