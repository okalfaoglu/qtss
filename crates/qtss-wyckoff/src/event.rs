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
    /// 2026-04-27 — Phase B intra-range Upthrust / Upthrust Action
    /// (UT / UA per Trading Wyckoff). Minor pierce-then-reject of
    /// the range high INSIDE Phase B (not the climactic Phase C
    /// UTAD which lands on heavier volume + actually triggers
    /// the trend reversal). Bull-side mirror = Phase B Spring at
    /// the range bottom — already covered by eval_spring with the
    /// relaxed wick gates.
    Ut,
    /// 2026-04-27 — minor Sign of Weakness inside Phase B
    /// ("mSOW" per Trading Wyckoff). A wide-range bear bar
    /// during the building-cause phase that doesn't yet break
    /// the range, signalling supply leaning into demand. The
    /// final Phase D SOW that DOES break is still emitted by
    /// eval_sow.
    Msow,
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
            Self::Ut => "ut",
            Self::Msow => "msow",
        }
    }
    pub fn is_distribution(self) -> bool {
        matches!(
            self,
            Self::Bc | Self::Utad | Self::Sow | Self::Ut | Self::Msow
        )
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
