//! Shared types for the Wyckoff catalog.

use crate::config::WyckoffConfig;
use qtss_domain::v2::bar::Bar;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffEventKind {
    Ps,
    Sc,
    Ar,
    St,
    Spring,
    Test,
    Sos,
    Lps,
    Bu,
    Bc,
    Utad,
    Sow,
}

impl WyckoffEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ps => "ps",
            Self::Sc => "sc",
            Self::Ar => "ar",
            Self::St => "st",
            Self::Spring => "spring",
            Self::Test => "test",
            Self::Sos => "sos",
            Self::Lps => "lps",
            Self::Bu => "bu",
            Self::Bc => "bc",
            Self::Utad => "utad",
            Self::Sow => "sow",
        }
    }
    pub fn is_distribution(self) -> bool {
        matches!(self, Self::Bc | Self::Utad | Self::Sow)
    }
    pub fn is_accumulation(self) -> bool {
        !self.is_distribution()
    }
}

#[derive(Debug, Clone)]
pub struct WyckoffEvent {
    pub kind: WyckoffEventKind,
    pub variant: &'static str, // "bull" / "bear"
    pub score: f64,
    pub bar_index: usize,
    pub reference_price: f64,
    pub volume_ratio: f64,
    pub range_ratio: f64,
    pub note: String,
}

pub struct WyckoffSpec {
    pub name: &'static str,
    pub kind: WyckoffEventKind,
    /// Evaluator signature: bars + config → 0..N events.
    pub eval: fn(&[Bar], &WyckoffConfig) -> Vec<WyckoffEvent>,
}
