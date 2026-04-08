//! Pattern detection contract — the common envelope every detector returns.
//!
//! See `docs/QTSS_V2_ARCHITECTURE_PLAN.md` §5. Detectors stay pure: they
//! report what they see and leave `confidence` + `targets` for the
//! validator and target-engine to fill in. This separation is enforced
//! by leaving those fields out of the constructor entry path so a
//! detector cannot accidentally produce them.

use crate::v2::instrument::Instrument;
use crate::v2::pivot::PivotLevel;
use crate::v2::regime::RegimeSnapshot;
use crate::v2::timeframe::Timeframe;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What kind of pattern was detected. Open enum: families share an outer
/// label and a `subkind` string so we don't have to recompile to add a
/// new harmonic variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "family", content = "subkind")]
pub enum PatternKind {
    Elliott(String),  // "impulse_5", "abc_zigzag", "diagonal", ...
    Harmonic(String), // "gartley", "butterfly", "bat", "crab", ...
    Classical(String),// "head_and_shoulders", "double_top", "wedge", ...
    Wyckoff(String),  // "accumulation", "spring", "sos", ...
    Range(String),    // "fvg", "order_block", "liquidity_pool", ...
    Custom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternState {
    Forming,
    Confirmed,
    Invalidated,
    Completed,
}

/// Reference to a pivot used as a structural anchor for the pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PivotRef {
    pub bar_index: u64,
    pub price: Decimal,
    pub level: PivotLevel,
    /// Optional human label, e.g. "X", "A", "B", "C", "D" for harmonic
    /// or "1", "2", "3", "4", "5" for Elliott impulse.
    pub label: Option<String>,
}

/// How a target was derived. Used by `qtss-target-engine` for clustering
/// and by the GUI for tooltips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetMethod {
    FibExtension,
    FibRetracement,
    MeasuredMove,
    HarmonicPrz,
    ElliottProjection,
    SupportResistance,
    Cluster,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub price: Decimal,
    pub method: TargetMethod,
    /// 0..1 — how strongly the engine believes in this level.
    pub weight: f32,
    pub label: Option<String>,
}

/// The shared output of every pattern detector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    pub id: Uuid,
    pub instrument: Instrument,
    pub timeframe: Timeframe,
    pub kind: PatternKind,
    pub state: PatternState,
    pub anchors: Vec<PivotRef>,
    /// Detector's own structural-rule score (e.g. how cleanly Fib ratios
    /// matched). 0..1. Validator combines this with historical hit rate
    /// to produce the final `confidence` on a `ValidatedDetection`.
    pub structural_score: f32,
    pub invalidation_price: Decimal,
    pub regime_at_detection: RegimeSnapshot,
    pub detected_at: DateTime<Utc>,
    /// Detector-specific extras (Fib ratios used, swing IDs, etc.).
    pub raw_meta: serde_json::Value,
}

impl Detection {
    /// Helper used by detector implementations.
    pub fn new(
        instrument: Instrument,
        timeframe: Timeframe,
        kind: PatternKind,
        state: PatternState,
        anchors: Vec<PivotRef>,
        structural_score: f32,
        invalidation_price: Decimal,
        regime: RegimeSnapshot,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            instrument,
            timeframe,
            kind,
            state,
            anchors,
            structural_score,
            invalidation_price,
            regime_at_detection: regime,
            detected_at: Utc::now(),
            raw_meta: serde_json::Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::instrument::{AssetClass, SessionCalendar, Venue};
    use crate::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};
    use rust_decimal_macros::dec;

    fn regime() -> RegimeSnapshot {
        RegimeSnapshot {
            at: Utc::now(),
            kind: RegimeKind::TrendingUp,
            trend_strength: TrendStrength::Strong,
            adx: dec!(30),
            bb_width: dec!(0.04),
            atr_pct: dec!(0.02),
            choppiness: dec!(40),
            confidence: 0.8,
        }
    }

    fn instrument() -> Instrument {
        Instrument {
            venue: Venue::Binance,
            asset_class: AssetClass::CryptoSpot,
            symbol: "BTCUSDT".into(),
            quote_ccy: "USDT".into(),
            tick_size: dec!(0.01),
            lot_size: dec!(0.00001),
            session: SessionCalendar::binance_24x7(),
        }
    }

    #[test]
    fn detection_round_trips_through_json() {
        let d = Detection::new(
            instrument(),
            Timeframe::H4,
            PatternKind::Harmonic("gartley".into()),
            PatternState::Forming,
            vec![],
            0.72,
            dec!(95.0),
            regime(),
        );
        let j = serde_json::to_string(&d).unwrap();
        let back: Detection = serde_json::from_str(&j).unwrap();
        assert_eq!(back.kind, PatternKind::Harmonic("gartley".into()));
        assert_eq!(back.state, PatternState::Forming);
    }

    #[test]
    fn pattern_kind_serializes_with_family_tag() {
        let k = PatternKind::Elliott("impulse_5".into());
        let j = serde_json::to_value(&k).unwrap();
        assert_eq!(j["family"], "elliott");
        assert_eq!(j["subkind"], "impulse_5");
    }
}
