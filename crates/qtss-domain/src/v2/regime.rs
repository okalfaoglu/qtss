//! Market regime classification snapshot.
//!
//! Produced by `qtss-regime` (future crate). Detectors and the confluence
//! aggregator subscribe to `regime.changed` events and adapt thresholds
//! accordingly — values themselves come from `qtss-config`, never hardcoded.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegimeKind {
    TrendingUp,
    TrendingDown,
    Ranging,
    Squeeze,
    Volatile,
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendStrength {
    None,
    Weak,
    Moderate,
    Strong,
    VeryStrong,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegimeSnapshot {
    pub at: DateTime<Utc>,
    pub kind: RegimeKind,
    pub trend_strength: TrendStrength,
    /// ADX value at the snapshot.
    pub adx: Decimal,
    /// Bollinger Band width (high - low) / mid.
    pub bb_width: Decimal,
    /// ATR / price ratio.
    pub atr_pct: Decimal,
    /// Choppiness Index, 0..100.
    pub choppiness: Decimal,
    /// Confidence in the classification, 0..1.
    pub confidence: f32,
}

impl RegimeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TrendingUp => "trending_up",
            Self::TrendingDown => "trending_down",
            Self::Ranging => "ranging",
            Self::Squeeze => "squeeze",
            Self::Volatile => "volatile",
            Self::Uncertain => "uncertain",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "trending_up" => Some(Self::TrendingUp),
            "trending_down" => Some(Self::TrendingDown),
            "ranging" => Some(Self::Ranging),
            "squeeze" => Some(Self::Squeeze),
            "volatile" => Some(Self::Volatile),
            "uncertain" => Some(Self::Uncertain),
            _ => None,
        }
    }
}

impl TrendStrength {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Weak => "weak",
            Self::Moderate => "moderate",
            Self::Strong => "strong",
            Self::VeryStrong => "very_strong",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "weak" => Some(Self::Weak),
            "moderate" => Some(Self::Moderate),
            "strong" => Some(Self::Strong),
            "very_strong" => Some(Self::VeryStrong),
            _ => None,
        }
    }
}

impl RegimeSnapshot {
    /// Fallback snapshot used when the stored regime JSON is empty or
    /// malformed (e.g. TBM detections that store `{}`). All values are
    /// neutral so the validator's regime-alignment channel abstains
    /// rather than penalising.
    pub fn neutral_default() -> Self {
        Self {
            at: Utc::now(),
            kind: RegimeKind::Uncertain,
            trend_strength: TrendStrength::None,
            adx: Decimal::ZERO,
            bb_width: Decimal::ZERO,
            atr_pct: Decimal::ZERO,
            choppiness: Decimal::ZERO,
            confidence: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn json_round_trip() {
        let s = RegimeSnapshot {
            at: Utc::now(),
            kind: RegimeKind::TrendingUp,
            trend_strength: TrendStrength::Strong,
            adx: dec!(34.5),
            bb_width: dec!(0.042),
            atr_pct: dec!(0.018),
            choppiness: dec!(38.0),
            confidence: 0.82,
        };
        let j = serde_json::to_string(&s).unwrap();
        let back: RegimeSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }
}
