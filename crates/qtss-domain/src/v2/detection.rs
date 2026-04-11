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

/// Elliott Wave degree label per Frost & Prechter.
///
/// Maps from timeframe resolution to the conventional degree name.
/// Used as a label only — does NOT change detection rules. A future
/// multi-degree validation phase will use this for cross-timeframe
/// consistency checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveDegree {
    GrandSupercycle,
    Supercycle,
    Cycle,
    Primary,
    Intermediate,
    Minor,
    Minute,
    Minuette,
    Subminuette,
}

impl WaveDegree {
    /// Map a timeframe to its default wave degree.
    ///
    /// This is a heuristic — the actual degree depends on how many bars
    /// the pattern spans, not just the chart resolution. A future phase
    /// will refine this with bar-count context.
    pub fn from_timeframe(tf: Timeframe) -> Self {
        match tf {
            Timeframe::Mn1             => WaveDegree::Primary,
            Timeframe::W1              => WaveDegree::Intermediate,
            Timeframe::D1 | Timeframe::D3 => WaveDegree::Minor,
            Timeframe::H4 | Timeframe::H6 | Timeframe::H8 | Timeframe::H12
                                       => WaveDegree::Minute,
            Timeframe::H1 | Timeframe::H2
                                       => WaveDegree::Minuette,
            Timeframe::M15 | Timeframe::M30
                                       => WaveDegree::Minuette,
            Timeframe::M1 | Timeframe::M3 | Timeframe::M5
                                       => WaveDegree::Subminuette,
        }
    }

    /// Conventional notation for impulse waves at this degree.
    pub fn impulse_notation(self) -> &'static [&'static str; 5] {
        match self {
            WaveDegree::GrandSupercycle => &["[1]", "[2]", "[3]", "[4]", "[5]"],
            WaveDegree::Supercycle      => &["(I)", "(II)", "(III)", "(IV)", "(V)"],
            WaveDegree::Cycle           => &["I", "II", "III", "IV", "V"],
            WaveDegree::Primary         => &["[1]", "[2]", "[3]", "[4]", "[5]"],
            WaveDegree::Intermediate    => &["(1)", "(2)", "(3)", "(4)", "(5)"],
            WaveDegree::Minor           => &["1", "2", "3", "4", "5"],
            WaveDegree::Minute          => &["[i]", "[ii]", "[iii]", "[iv]", "[v]"],
            WaveDegree::Minuette        => &["(i)", "(ii)", "(iii)", "(iv)", "(v)"],
            WaveDegree::Subminuette     => &["i", "ii", "iii", "iv", "v"],
        }
    }

    /// Conventional notation for corrective waves at this degree.
    pub fn corrective_notation(self) -> &'static [&'static str; 3] {
        match self {
            WaveDegree::GrandSupercycle => &["[a]", "[b]", "[c]"],
            WaveDegree::Supercycle      => &["(a)", "(b)", "(c)"],
            WaveDegree::Cycle           => &["a", "b", "c"],
            WaveDegree::Primary         => &["[A]", "[B]", "[C]"],
            WaveDegree::Intermediate    => &["(A)", "(B)", "(C)"],
            WaveDegree::Minor           => &["A", "B", "C"],
            WaveDegree::Minute          => &["[a]", "[b]", "[c]"],
            WaveDegree::Minuette        => &["(a)", "(b)", "(c)"],
            WaveDegree::Subminuette     => &["a", "b", "c"],
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            WaveDegree::GrandSupercycle => "Grand Supercycle",
            WaveDegree::Supercycle      => "Supercycle",
            WaveDegree::Cycle           => "Cycle",
            WaveDegree::Primary         => "Primary",
            WaveDegree::Intermediate    => "Intermediate",
            WaveDegree::Minor           => "Minor",
            WaveDegree::Minute          => "Minute",
            WaveDegree::Minuette        => "Minuette",
            WaveDegree::Subminuette     => "Subminuette",
        }
    }
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
    /// Forward-looking anchors the detector projects after the realized
    /// formation. For an Elliott impulse-in-progress this is the
    /// projected wave 4/5 path; for a completed structure it's the
    /// expected corrective leg. Empty when the detector has no
    /// projection (default-on-deserialize keeps existing JSON valid).
    #[serde(default)]
    pub projected_anchors: Vec<PivotRef>,
    /// Sub-wave decomposition: one inner vec per realized wave segment,
    /// holding the lower-degree pivots that fall *inside* that wave.
    /// Always either empty (decomposition not available) or has length
    /// `realized.len() - 1` so the chart can pair each sub-list with the
    /// matching higher-degree segment.
    #[serde(default)]
    pub sub_wave_anchors: Vec<Vec<PivotRef>>,
}

/// Output of `qtss-validator`. Wraps a `Detection` with the validator's
/// channel breakdown and the final blended confidence in `0..1`. Strategies
/// consume this — never the raw `Detection` — so confidence is the only
/// number a strategy needs to gate on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidatedDetection {
    pub detection: Detection,
    /// Per-channel scores produced by each confirmation channel that had
    /// an opinion. Channels that returned `None` are simply absent.
    pub channel_scores: Vec<ChannelScore>,
    /// Final blended confidence in `0..1`. Already includes the
    /// detector's `structural_score`.
    pub confidence: f32,
    pub validated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelScore {
    pub channel: String,
    pub score: f32,
    pub weight: f32,
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
            projected_anchors: Vec::new(),
            sub_wave_anchors: Vec::new(),
        }
    }

    /// Builder-style helper for detectors that compute a forward
    /// projection. Returns `self` so callers can chain `Detection::new(..)
    /// .with_projection(..)`.
    pub fn with_projection(mut self, projected: Vec<PivotRef>) -> Self {
        self.projected_anchors = projected;
        self
    }

    /// Builder-style helper for detectors that emit a sub-wave
    /// decomposition (one inner vec per realized segment).
    pub fn with_sub_waves(mut self, sub: Vec<Vec<PivotRef>>) -> Self {
        self.sub_wave_anchors = sub;
        self
    }

    /// Builder-style helper to set raw_meta JSON.
    pub fn with_meta(mut self, meta: serde_json::Value) -> Self {
        self.raw_meta = meta;
        self
    }

    /// Inject Elliott wave degree label into raw_meta based on timeframe.
    /// Only applies to `PatternKind::Elliott` — no-op for other families.
    pub fn with_degree(mut self) -> Self {
        if matches!(self.kind, PatternKind::Elliott(_)) {
            let degree = WaveDegree::from_timeframe(self.timeframe);
            let meta = match self.raw_meta {
                serde_json::Value::Object(mut map) => {
                    map.insert("degree".into(), serde_json::json!(degree));
                    map.insert("degree_label".into(), serde_json::json!(degree.label()));
                    serde_json::Value::Object(map)
                }
                serde_json::Value::Null => {
                    serde_json::json!({
                        "degree": degree,
                        "degree_label": degree.label()
                    })
                }
                other => {
                    serde_json::json!({
                        "previous": other,
                        "degree": degree,
                        "degree_label": degree.label()
                    })
                }
            };
            self.raw_meta = meta;
        }
        self
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
