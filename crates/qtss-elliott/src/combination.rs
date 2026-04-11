//! Combination correction (W-X-Y, W-X-Y-X-Z).
//!
//! A combination chains two (or three) simpler corrections separated by
//! connecting X-waves. Per Frost & Prechter the W and Y legs can be any
//! simple correction (zigzag, flat), Y may also be a triangle (the
//! terminal pattern of the combination). Z (triple combo) follows the
//! same logic with a second X and a third correction.
//!
//! **Pivot-tape approach**: instead of relying on prior detections from
//! the repository (cross-scan), we directly scan the pivot tape for
//! back-to-back corrective legs connected by linking moves:
//!
//!   W (4 pivots) + X (shared end → 1 extra) + Y (4 pivots shared start)
//!   = minimum 7 distinct pivots for W-X-Y.
//!
//! We try both 4+4 (zigzag/flat + zigzag/flat) and 4+6 (simple + triangle)
//! windows, picking the highest-scoring combination.

use crate::common::{alternation_ok, mean_score, nearest_fib_score};
use crate::config::ElliottConfig;
use crate::error::ElliottResult;
use crate::formation::FormationDetector;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState, PivotRef};
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::{Pivot, PivotTree};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal::prelude::ToPrimitive;

// Fib references for X-wave retracement (typically retraces 38.2–78.6% of W).
const X_REFS: &[f64] = &[0.382, 0.5, 0.618, 0.786];

// B-retrace refs for zigzag legs.
const ZZ_B_REFS: &[f64] = &[0.5, 0.618, 0.786];
// C-extension refs for zigzag legs.
const ZZ_C_REFS: &[f64] = &[1.0, 1.272, 1.618];

// B-retrace refs for flat legs (B ≥ ~85% of A).
const FLAT_B_REFS: &[f64] = &[1.0, 1.05, 1.272];

pub struct CombinationDetector {
    config: ElliottConfig,
}

impl CombinationDetector {
    pub fn new(config: ElliottConfig) -> ElliottResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }
}

// ---------------------------------------------------------------------------
// Leg scoring helpers
// ---------------------------------------------------------------------------

/// Score a 4-pivot slice as a zigzag (5-3-5). Returns None if invalid.
fn score_zigzag(raw: &[f64]) -> Option<f64> {
    if raw.len() < 4 { return None; }
    let a = raw[1] - raw[0];
    let b = raw[2] - raw[1];
    let c = raw[3] - raw[2];
    if a == 0.0 || b == 0.0 || c == 0.0 { return None; }
    if a.signum() != c.signum() { return None; }
    if b.signum() == a.signum() { return None; }

    let b_ret = b.abs() / a.abs();
    if b_ret > 0.95 { return None; } // too deep → flat territory

    // C must extend beyond A end.
    let c_beyond = if a < 0.0 { raw[3] < raw[1] } else { raw[3] > raw[1] };
    if !c_beyond { return None; }

    let c_ext = c.abs() / a.abs();
    let s_b = nearest_fib_score(b_ret, ZZ_B_REFS);
    let s_c = nearest_fib_score(c_ext, ZZ_C_REFS);
    Some(mean_score(&[s_b, s_c]))
}

/// Score a 4-pivot slice as a flat (3-3-5). Returns None if invalid.
fn score_flat(raw: &[f64]) -> Option<f64> {
    if raw.len() < 4 { return None; }
    let a = raw[1] - raw[0];
    let b = raw[2] - raw[1];
    let c = raw[3] - raw[2];
    if a == 0.0 || c == 0.0 { return None; }
    if a.signum() != c.signum() { return None; }
    if b.signum() == a.signum() { return None; }

    let b_ratio = b.abs() / a.abs();
    if b_ratio < 0.85 { return None; } // not flat-like

    let s_b = nearest_fib_score(b_ratio, FLAT_B_REFS);
    // C vs A ratio — flats have C ≈ 1.0×A for regular, up to 1.618× for expanded.
    let c_ratio = c.abs() / a.abs();
    let s_c = nearest_fib_score(c_ratio, &[1.0, 1.272, 1.618]);
    Some(mean_score(&[s_b, s_c]))
}

/// Score a simple corrective leg (best of zigzag / flat).
fn score_simple_leg(raw: &[f64]) -> Option<(&'static str, f64)> {
    let zz = score_zigzag(raw);
    let fl = score_flat(raw);
    match (zz, fl) {
        (Some(a), Some(b)) if a >= b => Some(("zigzag", a)),
        (Some(a), Some(b)) if b > a  => Some(("flat", b)),
        (Some(a), None)               => Some(("zigzag", a)),
        (None, Some(b))               => Some(("flat", b)),
        _                              => None,
    }
}

/// Score the X-wave as a retracement of the W leg.
fn score_x_wave(w_start: f64, w_end: f64, x_end: f64) -> Option<f64> {
    let w_range = w_end - w_start;
    if w_range == 0.0 { return None; }
    let x_retrace = (x_end - w_end).abs() / w_range.abs();
    // X should retrace between 20% and 90% of W.
    if x_retrace < 0.15 || x_retrace > 0.95 { return None; }
    Some(nearest_fib_score(x_retrace, X_REFS))
}

// ---------------------------------------------------------------------------
// Candidate structure
// ---------------------------------------------------------------------------

struct ComboCandidate {
    start: usize,   // index into pivots slice
    w_type: &'static str,
    y_type: &'static str,
    score: f64,
    pivot_count: usize, // 7 for simple W-X-Y
}

impl FormationDetector for CombinationDetector {
    fn name(&self) -> &'static str {
        "combination"
    }

    fn detect(
        &self,
        tree: &PivotTree,
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        let pivots = tree.at_level(self.config.pivot_level);
        // W(4) + X(1 extra) + Y(4, sharing pivot[4] as start) = 8 pivots.
        if pivots.len() < 8 {
            return Vec::new();
        }

        let mut best: Option<ComboCandidate> = None;

        for start in 0..=(pivots.len() - 8) {
            let window = &pivots[start..start + 8];
            if !alternation_ok(window) {
                continue;
            }
            if let Some(cand) = try_wxy_simple(window, start) {
                if best.as_ref().map(|b| cand.score > b.score).unwrap_or(true) {
                    best = Some(cand);
                }
            }
        }

        let Some(cand) = best else {
            return Vec::new();
        };
        if (cand.score as f32) < self.config.min_structural_score {
            return Vec::new();
        }

        let window = &pivots[cand.start..cand.start + cand.pivot_count];
        let raw: Vec<f64> = window.iter().map(|p| p.price.to_f64().unwrap_or(0.0)).collect();

        // Direction: the overall combination corrects the prior trend.
        // If W goes down (start HIGH) → correcting bullish → bear.
        let w_leg = raw[3] - raw[0];
        let suffix = if w_leg < 0.0 { "bear" } else { "bull" };
        let subkind = format!(
            "combination_wxy_{}_{}_{suffix}",
            cand.w_type, cand.y_type
        );

        let anchors = build_wxy_anchors(window, self.config.pivot_level);

        // Invalidation: W start — if price reverses past W's origin the
        // entire corrective structure is broken.
        let invalidation_price = window[0].price;

        vec![Detection::new(
            instrument.clone(),
            timeframe,
            PatternKind::Elliott(subkind),
            PatternState::Forming,
            anchors,
            cand.score as f32,
            invalidation_price,
            regime.clone(),
        )]
    }
}

/// Try to interpret an 8-pivot window as W(4) + X(leg) + Y(4).
///
/// Layout (8 pivots):
///   W  = pivots[0..4]  (indices 0, 1, 2, 3)
///   X  = connecting leg from pivot[3] → pivot[4]
///   Y  = pivots[4..8]  (indices 4, 5, 6, 7)
///
/// Both W and Y are scored as simple corrections (zigzag or flat).
/// The X wave must retrace part of W's range (typically 38–79%).
/// Y must continue in the same overall direction as W.
fn try_wxy_simple(window: &[Pivot], start: usize) -> Option<ComboCandidate> {
    if window.len() < 8 {
        return None;
    }

    let raw: Vec<f64> = window.iter().map(|p| p.price.to_f64().unwrap_or(0.0)).collect();

    // Score W leg (pivots 0-3).
    let (w_type, w_score) = score_simple_leg(&raw[0..4])?;

    // Score X wave (pivot 3 → 4).
    let x_score = score_x_wave(raw[0], raw[3], raw[4])?;

    // The X wave must retrace W — i.e. X goes in opposite direction to W.
    let w_dir = (raw[3] - raw[0]).signum();
    let x_dir = (raw[4] - raw[3]).signum();
    if w_dir == x_dir || w_dir == 0.0 || x_dir == 0.0 {
        return None;
    }

    // Score Y leg (pivots 4-7).
    let (y_type, y_score) = score_simple_leg(&raw[4..8])?;

    // Y must move in the same overall direction as W.
    let y_dir = (raw[7] - raw[4]).signum();
    if y_dir != w_dir {
        return None;
    }

    // Y ≈ 0.618–1.618 × W range — reward canonical equality ratios.
    let w_range = (raw[3] - raw[0]).abs();
    let y_range = (raw[7] - raw[4]).abs();
    let y_w_ratio = if w_range > 0.0 { y_range / w_range } else { 0.0 };
    let s_equality = nearest_fib_score(y_w_ratio, &[0.618, 1.0, 1.272, 1.618]);

    let score = mean_score(&[w_score, x_score, y_score, s_equality]);

    Some(ComboCandidate {
        start,
        w_type,
        y_type,
        score,
        pivot_count: 8,
    })
}

/// Build labelled anchors for W-X-Y (8 pivots).
fn build_wxy_anchors(window: &[Pivot], level: qtss_domain::v2::pivot::PivotLevel) -> Vec<PivotRef> {
    const LABELS: &[&str] = &["W", "W-A", "W-B", "W-C", "X/Y", "Y-A", "Y-B", "Y-C"];
    window.iter()
        .zip(LABELS.iter())
        .map(|(p, l)| PivotRef {
            bar_index: p.bar_index,
            price: p.price,
            level,
            label: Some((*l).to_string()),
        })
        .collect()
}
