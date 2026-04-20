//! `/v2/chart/{venue}/{symbol}/{tf}` wire types -- Faz 5 Adim (b).
//!
//! Returns a single payload the chart workspace can render in one round
//! trip: candles, renko bricks, active detections (patterns), open
//! positions and open orders. Each list is independently optional --
//! frontend toggles candle vs renko, but the backend always sends both
//! when present so the user can flip without a refetch.
//!
//! Renko conversion lives next to the DTOs (not in qtss-indicators) on
//! purpose: the brick-size policy is GUI-side -- it controls what the
//! chart panel shows, not what detectors compute on. Keeping it here
//! avoids dragging chart concerns into the analysis stack.

use crate::dashboard::OpenPositionView;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// One OHLCV candle. Wire-shaped (no DB ids, no segment metadata) --
/// the route already knows what venue/symbol/timeframe it returned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandleBar {
    pub open_time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
}

/// One renko brick. `direction` is `+1` for up, `-1` for down.
/// `at` is the close time of the source candle that completed the
/// brick, so the chart can place it on the time axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenkoBrick {
    pub at: DateTime<Utc>,
    pub open: Decimal,
    pub close: Decimal,
    pub direction: i8,
}

/// One pivot inside a detection's anchor chain. The frontend connects
/// these in order with a polyline so the pattern geometry is visible
/// (impulse waves, triangle apex, double-bottom necks, …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectionAnchor {
    pub time: DateTime<Utc>,
    pub price: Decimal,
    /// Optional label for the pivot — Elliott wave number, harmonic
    /// point letter, etc. Kept optional so patterns without per-pivot
    /// labels (e.g. range boundaries) don't have to invent one.
    pub label: Option<String>,
}

/// One detection (pattern) overlay row. Carries everything the chart
/// needs to draw the geometry without a second round-trip: anchor
/// chain, family/subkind for color coding, lifecycle state, blended
/// confidence, and the invalidation level so we can render the stop
/// line. Channel-score detail still lives behind a click-through
/// endpoint to keep the wire payload bounded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectionOverlay {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub family: String,
    pub subkind: String,
    pub state: String,
    pub anchor_time: DateTime<Utc>,
    pub anchor_price: Decimal,
    pub confidence: Decimal,
    pub invalidation_price: Decimal,
    pub anchors: Vec<DetectionAnchor>,
    /// Forward-projected anchors (Faz 7.6 / A2). Same shape as
    /// `anchors`, but rendered with a dashed stroke. Empty when the
    /// detector emitted no projection.
    #[serde(default)]
    pub projected_anchors: Vec<DetectionAnchor>,
    /// Sub-wave decomposition (Faz 7.6 / A3). One inner vec per
    /// realized segment. Rendered fainter / thinner.
    #[serde(default)]
    pub sub_wave_anchors: Vec<Vec<DetectionAnchor>>,
    /// Elliott Deep: degree breadcrumb from wave_chain ancestor walk.
    /// e.g. "Cycle III > Primary [3] > Intermediate (3)"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wave_context: Option<String>,
    /// True when this detection has sub-waves on a lower timeframe
    /// (wave_chain children exist). Enables drill-down UI.
    #[serde(default)]
    pub has_children: bool,
    /// Aşama 5 — explicit overlay geometry `{ kind, payload }`. When
    /// present the frontend RENDER_KIND_REGISTRY dispatches on
    /// `kind`; when None the chart keeps the legacy anchor path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_geometry: Option<serde_json::Value>,
    /// Aşama 5 — family/variant style key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_style: Option<String>,
    /// Aşama 5 — anchor/leg label notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_labels: Option<serde_json::Value>,
    /// Faz 12 — which cascaded pivot level this detection came from.
    /// `"L0".."L3"` for harmonic/classical/elliott; `None` for TBM and
    /// other non-pivot-anchored families. The chart uses this to bind
    /// overlay visibility to the L0–L3 toggle buttons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pivot_level: Option<String>,
    /// Faz 12 — `"live" | "dry" | "backtest"`. Frontend filters by this
    /// so backtest sweep detections don't leak into the live view
    /// unless the operator explicitly asks for them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Faz 12.R — outcome evaluator results for this detection (only
    /// populated for `mode='backtest'` rows that have been evaluated).
    /// Used by the chart's info panel + state-based color tinting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,           // "win" | "loss" | "expired"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_pnl_pct: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_entry_price: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_exit_price: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_close_reason: Option<String>, // "tp_hit" | "sl_hit" | "time_stop"
    /// Faz 13 — `raw_meta.targets` projection for families that produce
    /// explicit A (R-multiple) + B (Fib) target packs (currently
    /// `pivot_reversal`). Shape: `{ a: {...}, b: {...} }`. Used by the
    /// chart to draw Fib overlay price lines when a detection is
    /// selected. Other families leave this None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targets: Option<serde_json::Value>,
}

/// One resting/working order overlay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenOrderOverlay {
    pub id: String,
    pub side: String,
    pub kind: String,
    pub price: Option<Decimal>,
    pub stop_price: Option<Decimal>,
    pub quantity: Decimal,
    pub status: String,
}

/// Whole `/v2/chart/...` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartWorkspace {
    pub generated_at: DateTime<Utc>,
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    pub candles: Vec<CandleBar>,
    pub renko: Vec<RenkoBrick>,
    pub detections: Vec<DetectionOverlay>,
    pub positions: Vec<OpenPositionView>,
    pub open_orders: Vec<OpenOrderOverlay>,
}

/// Pure renko converter. Brick size is given in absolute price units
/// (the route resolves it from `system_config` -- either a fixed pip
/// or `last_close * pct`, never hardcoded here per CLAUDE.md #2).
///
/// Algorithm: walk candles in time order, anchor at first close, emit
/// bricks whenever |close - anchor| crosses one brick. Direction
/// changes require *two* bricks of distance (classic renko reversal
/// rule) to suppress noise.
pub fn build_renko(candles: &[CandleBar], brick_size: Decimal) -> Vec<RenkoBrick> {
    if brick_size <= Decimal::ZERO || candles.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut anchor = candles[0].close;
    // 0 = no direction yet, 1 = up, -1 = down
    let mut last_dir: i8 = 0;
    for c in candles {
        loop {
            let diff = c.close - anchor;
            let need = if last_dir == 0 || last_dir == 1 {
                brick_size
            } else {
                brick_size + brick_size
            };
            if diff >= need {
                let open = anchor;
                let close = open + brick_size;
                out.push(RenkoBrick { at: c.open_time, open, close, direction: 1 });
                anchor = close;
                last_dir = 1;
                continue;
            }
            let need_down = if last_dir == 0 || last_dir == -1 {
                brick_size
            } else {
                brick_size + brick_size
            };
            if -diff >= need_down {
                let open = anchor;
                let close = open - brick_size;
                out.push(RenkoBrick { at: c.open_time, open, close, direction: -1 });
                anchor = close;
                last_dir = -1;
                continue;
            }
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn c(t: i64, close: Decimal) -> CandleBar {
        CandleBar {
            open_time: DateTime::<Utc>::from_timestamp(t, 0).unwrap(),
            open: close,
            high: close,
            low: close,
            close,
            volume: dec!(0),
        }
    }

    #[test]
    fn renko_emits_up_bricks_on_steady_climb() {
        let bars = vec![
            c(0, dec!(100)),
            c(1, dec!(101)),
            c(2, dec!(102)),
            c(3, dec!(103)),
        ];
        let bricks = build_renko(&bars, dec!(1));
        assert_eq!(bricks.len(), 3);
        assert!(bricks.iter().all(|b| b.direction == 1));
    }

    #[test]
    fn renko_reversal_needs_double_brick_distance() {
        // climb 100 -> 102 (two up bricks), then drop only 1 brick
        // (102 -> 101): the classic 2-brick reversal rule means
        // nothing should fire on the way down.
        let bars = vec![c(0, dec!(100)), c(1, dec!(102)), c(2, dec!(101))];
        let bricks = build_renko(&bars, dec!(1));
        assert_eq!(bricks.iter().filter(|b| b.direction == 1).count(), 2);
        assert_eq!(bricks.iter().filter(|b| b.direction == -1).count(), 0);
    }

    #[test]
    fn renko_reversal_fires_on_two_bricks_down() {
        // 100 -> 102 (two up bricks), then 100: a full 2-brick drop
        // exactly meets the reversal threshold and emits a down brick.
        let bars = vec![c(0, dec!(100)), c(1, dec!(102)), c(2, dec!(100))];
        let bricks = build_renko(&bars, dec!(1));
        assert!(bricks.iter().any(|b| b.direction == -1));
    }

    #[test]
    fn renko_zero_brick_returns_empty() {
        let bars = vec![c(0, dec!(100)), c(1, dec!(101))];
        assert!(build_renko(&bars, dec!(0)).is_empty());
    }
}
