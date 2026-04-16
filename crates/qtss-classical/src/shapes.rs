//! Classical chart-pattern catalog.
//!
//! Each entry is a [`ShapeSpec`]: a name, the number of trailing pivots
//! it consumes, and an `eval` function pointer. The detector walks every
//! spec through the same loop and keeps the highest-scoring match — no
//! central match arm to edit when adding a new pattern (CLAUDE.md #1).
//!
//! Each `eval` receives the trailing pivots (oldest..newest), the
//! [`ClassicalConfig`] (so eval has access to tolerances, all configurable,
//! no hardcoded magic per CLAUDE.md #2) and returns a [`ShapeMatch`] when
//! the geometry passes its rules.

use crate::config::ClassicalConfig;
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::pivot::{Pivot, PivotKind};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

/// Successful pattern match.
#[derive(Debug, Clone)]
pub struct ShapeMatch {
    /// Structural quality, 0..1. Higher = cleaner geometry.
    pub score: f64,
    /// Price beyond which the pattern is geometrically broken.
    pub invalidation: Decimal,
    /// Anchor labels assigned to the trailing pivots, oldest..newest.
    pub anchor_labels: Vec<&'static str>,
    /// Pattern variant suffix, e.g. "bull"/"bear"/"top"/"bottom"/"asc".
    /// Joined with the spec name to form the final `PatternKind` string.
    pub variant: &'static str,
}

pub struct ShapeSpec {
    pub name: &'static str,
    pub pivots_needed: usize,
    pub eval: fn(&[Pivot], &ClassicalConfig) -> Option<ShapeMatch>,
}

pub const SHAPES: &[ShapeSpec] = &[
    ShapeSpec {
        name: "double_top",
        pivots_needed: 3,
        eval: eval_double_top,
    },
    ShapeSpec {
        name: "double_bottom",
        pivots_needed: 3,
        eval: eval_double_bottom,
    },
    ShapeSpec {
        name: "head_and_shoulders",
        pivots_needed: 5,
        eval: eval_head_and_shoulders,
    },
    ShapeSpec {
        name: "inverse_head_and_shoulders",
        pivots_needed: 5,
        eval: eval_inverse_head_and_shoulders,
    },
    ShapeSpec {
        name: "ascending_triangle",
        pivots_needed: 4,
        eval: eval_ascending_triangle,
    },
    ShapeSpec {
        name: "descending_triangle",
        pivots_needed: 4,
        eval: eval_descending_triangle,
    },
    ShapeSpec {
        name: "symmetrical_triangle",
        pivots_needed: 4,
        eval: eval_symmetrical_triangle,
    },
    // P5.1 — Rectangle (flat top + flat bottom, continuation or reversal
    // depending on the prior trend; detector is direction-neutral, the
    // breakout channel on the validator decides the side).
    ShapeSpec {
        name: "rectangle",
        pivots_needed: 4,
        eval: eval_rectangle,
    },
    // P5.3 — Wedge (rising = bearish, falling = bullish). Both
    // trendlines slope the same direction and converge toward an apex.
    // Distinct from triangles where slopes oppose (sym) or one is flat
    // (asc/desc).
    ShapeSpec {
        name: "rising_wedge",
        pivots_needed: 4,
        eval: eval_rising_wedge,
    },
    ShapeSpec {
        name: "falling_wedge",
        pivots_needed: 4,
        eval: eval_falling_wedge,
    },
    // P5.4 — Price Channel (paralel iki çizgi, trendli). Asc → bull,
    // desc → bear. Rectangle'dan farkı: çizgiler eğimli (trend var).
    ShapeSpec {
        name: "ascending_channel",
        pivots_needed: 4,
        eval: eval_ascending_channel,
    },
    ShapeSpec {
        name: "descending_channel",
        pivots_needed: 4,
        eval: eval_descending_channel,
    },
    // P5.6 — Diamond top (bear) / bottom (bull). Sol yarı genişleyen,
    // sağ yarı daralan; 6 alternatif pivot ile temsil edilir.
    ShapeSpec {
        name: "diamond_top",
        pivots_needed: 6,
        eval: eval_diamond_top,
    },
    ShapeSpec {
        name: "diamond_bottom",
        pivots_needed: 6,
        eval: eval_diamond_bottom,
    },
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn p(pivot: &Pivot) -> Option<f64> {
    pivot.price.to_f64()
}

fn require_kinds(pivots: &[Pivot], expected: &[PivotKind]) -> bool {
    pivots.len() == expected.len()
        && pivots.iter().zip(expected.iter()).all(|(p, k)| p.kind == *k)
}

/// Closeness score for two values that should be approximately equal.
/// Returns 1.0 when identical, 0.0 at `tolerance` apart, Gaussian fall-off.
fn equality_score(a: f64, b: f64, tolerance: f64) -> Option<f64> {
    let mid = (a.abs() + b.abs()) / 2.0;
    if mid <= 0.0 {
        return None;
    }
    let diff = (a - b).abs() / mid;
    if diff > tolerance {
        return None;
    }
    let z = diff / tolerance.max(1e-9);
    Some((-(z * z)).exp())
}

// ---------------------------------------------------------------------------
// Double top / bottom
// ---------------------------------------------------------------------------

fn eval_double_top(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    // pattern: H1 (high)  T (low)  H2 (high), H1 ~= H2.
    if !require_kinds(pivots, &[PivotKind::High, PivotKind::Low, PivotKind::High]) {
        return None;
    }
    let h1 = p(&pivots[0])?;
    let t = p(&pivots[1])?;
    let h2 = p(&pivots[2])?;
    if t >= h1.min(h2) {
        return None;
    }
    let score = equality_score(h1, h2, cfg.equality_tolerance)?;
    Some(ShapeMatch {
        score,
        // top breaks when price closes above the higher peak
        invalidation: pivots[0].price.max(pivots[2].price),
        anchor_labels: vec!["H1", "T", "H2"],
        variant: "bear",
    })
}

fn eval_double_bottom(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    if !require_kinds(pivots, &[PivotKind::Low, PivotKind::High, PivotKind::Low]) {
        return None;
    }
    let l1 = p(&pivots[0])?;
    let t = p(&pivots[1])?;
    let l2 = p(&pivots[2])?;
    if t <= l1.max(l2) {
        return None;
    }
    let score = equality_score(l1, l2, cfg.equality_tolerance)?;
    Some(ShapeMatch {
        score,
        invalidation: pivots[0].price.min(pivots[2].price),
        anchor_labels: vec!["L1", "T", "L2"],
        variant: "bull",
    })
}

// ---------------------------------------------------------------------------
// Head & Shoulders
// ---------------------------------------------------------------------------
//
// classic top: LS(high) N1(low) H(high) N2(low) RS(high)
//   - H > LS, H > RS
//   - LS ~= RS
//   - N1 ~= N2 (neckline roughly horizontal)
//
// inverse: mirror.

/// P3 — neckline slope score. Returns None if slope exceeds the cap
/// (pattern rejected), otherwise a 0..1 score that decays linearly from
/// 1.0 at perfectly horizontal to 0.0 at the cap.
fn neckline_slope_score(
    n1: f64,
    n1_bar: u64,
    n2: f64,
    n2_bar: u64,
    cap_pct: f64,
) -> Option<f64> {
    let dx = (n2_bar as i64 - n1_bar as i64).abs() as f64;
    if dx <= 0.0 {
        return Some(1.0);
    }
    let mid = (n1.abs() + n2.abs()) / 2.0;
    if mid <= 0.0 {
        return None;
    }
    let slope_pct_per_bar = ((n2 - n1).abs() / mid) / dx;
    if slope_pct_per_bar > cap_pct {
        return None; // reject outright — neckline too steep
    }
    Some(1.0 - (slope_pct_per_bar / cap_pct).clamp(0.0, 1.0))
}

/// P3 — shoulder time-symmetry score. Returns None when imbalance
/// exceeds `tol` (pattern rejected), otherwise a 0..1 score.
fn shoulder_time_symmetry_score(
    ls_bar: u64,
    h_bar: u64,
    rs_bar: u64,
    tol: f64,
) -> Option<f64> {
    let left = (h_bar as i64 - ls_bar as i64).abs() as f64;
    let right = (rs_bar as i64 - h_bar as i64).abs() as f64;
    if left <= 0.0 || right <= 0.0 {
        return None;
    }
    let avg = (left + right) / 2.0;
    let imbalance = (left - right).abs() / avg;
    if imbalance > tol {
        return None;
    }
    Some(1.0 - (imbalance / tol).clamp(0.0, 1.0))
}

fn eval_head_and_shoulders(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    if !require_kinds(
        pivots,
        &[
            PivotKind::High,
            PivotKind::Low,
            PivotKind::High,
            PivotKind::Low,
            PivotKind::High,
        ],
    ) {
        return None;
    }
    let ls = p(&pivots[0])?;
    let n1 = p(&pivots[1])?;
    let h = p(&pivots[2])?;
    let n2 = p(&pivots[3])?;
    let rs = p(&pivots[4])?;
    if !(h > ls && h > rs) {
        return None;
    }
    let s_shoulders = equality_score(ls, rs, cfg.equality_tolerance)?;
    let s_neck = equality_score(n1, n2, cfg.equality_tolerance * cfg.neckline_tolerance_mult)?;
    // P3 — slope cap + time symmetry. Both return None (pattern rejected)
    // if out of bounds; otherwise contribute a 0..1 score to the blend.
    let s_slope = neckline_slope_score(
        n1, pivots[1].bar_index, n2, pivots[3].bar_index, cfg.hs_max_neckline_slope_pct,
    )?;
    let s_time = shoulder_time_symmetry_score(
        pivots[0].bar_index, pivots[2].bar_index, pivots[4].bar_index, cfg.hs_time_symmetry_tol,
    )?;
    Some(ShapeMatch {
        score: (s_shoulders + s_neck + s_slope + s_time) / 4.0,
        invalidation: pivots[2].price, // head break
        anchor_labels: vec!["LS", "N1", "H", "N2", "RS"],
        variant: "bear",
    })
}

fn eval_inverse_head_and_shoulders(
    pivots: &[Pivot],
    cfg: &ClassicalConfig,
) -> Option<ShapeMatch> {
    if !require_kinds(
        pivots,
        &[
            PivotKind::Low,
            PivotKind::High,
            PivotKind::Low,
            PivotKind::High,
            PivotKind::Low,
        ],
    ) {
        return None;
    }
    let ls = p(&pivots[0])?;
    let n1 = p(&pivots[1])?;
    let h = p(&pivots[2])?;
    let n2 = p(&pivots[3])?;
    let rs = p(&pivots[4])?;
    if !(h < ls && h < rs) {
        return None;
    }
    let s_shoulders = equality_score(ls, rs, cfg.equality_tolerance)?;
    let s_neck = equality_score(n1, n2, cfg.equality_tolerance * cfg.neckline_tolerance_mult)?;
    let s_slope = neckline_slope_score(
        n1, pivots[1].bar_index, n2, pivots[3].bar_index, cfg.hs_max_neckline_slope_pct,
    )?;
    let s_time = shoulder_time_symmetry_score(
        pivots[0].bar_index, pivots[2].bar_index, pivots[4].bar_index, cfg.hs_time_symmetry_tol,
    )?;
    Some(ShapeMatch {
        score: (s_shoulders + s_neck + s_slope + s_time) / 4.0,
        invalidation: pivots[2].price,
        anchor_labels: vec!["LS", "N1", "H", "N2", "RS"],
        variant: "bull",
    })
}

// ---------------------------------------------------------------------------
// Triangles (4 pivots = two highs + two lows alternating)
// ---------------------------------------------------------------------------
//
// We accept either ordering (HLHL or LHLH) and compare the slopes of the
// upper trendline (joining the two highs) and lower trendline (joining
// the two lows).
//
// ascending  : upper slope ~ 0,   lower slope > 0
// descending : upper slope < 0,   lower slope ~ 0
// symmetrical: upper slope < 0,   lower slope > 0  (converging)
//
// The convergence apex (where the two lines meet) must lie within
// `apex_horizon_bars` of the last pivot — otherwise it is too open to
// be a triangle.

#[derive(Debug, Clone, Copy)]
struct Line {
    slope: f64,
    intercept: f64,
}

fn line_from(p1: (f64, f64), p2: (f64, f64)) -> Option<Line> {
    let dx = p2.0 - p1.0;
    if dx.abs() < f64::EPSILON {
        return None;
    }
    let slope = (p2.1 - p1.1) / dx;
    let intercept = p1.1 - slope * p1.0;
    Some(Line { slope, intercept })
}

fn intersect_x(a: Line, b: Line) -> Option<f64> {
    let d = a.slope - b.slope;
    if d.abs() < f64::EPSILON {
        return None;
    }
    Some((b.intercept - a.intercept) / d)
}

/// Returns ((upper_line, lower_line), last_bar_index) for the four pivots
/// regardless of whether they alternate HLHL or LHLH.
fn triangle_lines(pivots: &[Pivot]) -> Option<(Line, Line, u64)> {
    if pivots.len() != 4 {
        return None;
    }
    let mut highs: Vec<(f64, f64)> = Vec::new();
    let mut lows: Vec<(f64, f64)> = Vec::new();
    for piv in pivots {
        let y = p(piv)?;
        let x = piv.bar_index as f64;
        match piv.kind {
            PivotKind::High => highs.push((x, y)),
            PivotKind::Low => lows.push((x, y)),
        }
    }
    if highs.len() != 2 || lows.len() != 2 {
        return None;
    }
    let upper = line_from(highs[0], highs[1])?;
    let lower = line_from(lows[0], lows[1])?;
    let last = pivots.iter().map(|p| p.bar_index).max().unwrap_or(0);
    Some((upper, lower, last))
}

fn flatness_score(slope: f64, reference_price: f64, threshold_pct: f64) -> f64 {
    // Slope is in price-per-bar; normalise by reference price to get a
    // unitless "% change per bar". Below `threshold_pct` per bar counts
    // as effectively flat. Gaussian fall-off thereafter. Threshold is
    // config-driven (CLAUDE.md #2).
    if reference_price <= 0.0 || threshold_pct <= 0.0 {
        return 0.0;
    }
    let pct = (slope / reference_price).abs();
    let z = pct / threshold_pct;
    (-(z * z) / 2.0).exp()
}

fn apex_score(upper: Line, lower: Line, last_bar: u64, horizon: u64) -> Option<f64> {
    let apex_x = intersect_x(upper, lower)?;
    let dx = apex_x - last_bar as f64;
    if dx <= 0.0 || dx > horizon as f64 {
        return None;
    }
    // Best score when apex is in the middle of the horizon, fading at edges.
    let normalised = dx / horizon as f64;
    Some(1.0 - (normalised - 0.5).abs() * 2.0 * 0.5) // scales 0.5..1.0
}

fn eval_ascending_triangle(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    let (upper, lower, last_bar) = triangle_lines(pivots)?;
    let ref_price = p(&pivots[pivots.len() - 1])?.abs();
    if !(upper.slope.abs() < lower.slope.abs() && lower.slope > 0.0) {
        return None;
    }
    let s_flat = flatness_score(upper.slope, ref_price, cfg.flatness_threshold_pct);
    let s_apex = apex_score(upper, lower, last_bar, cfg.apex_horizon_bars)?;
    if s_flat < cfg.flatness_min_score {
        return None;
    }
    Some(ShapeMatch {
        score: (s_flat + s_apex) / 2.0,
        // resistance break invalidates the wait-for-breakout setup; keep
        // the lowest pivot price as a structural floor for the validator.
        invalidation: pivots
            .iter()
            .map(|p| p.price)
            .min()
            .unwrap_or(Decimal::ZERO),
        anchor_labels: vec!["P1", "P2", "P3", "P4"],
        variant: "bull",
    })
}

fn eval_descending_triangle(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    let (upper, lower, last_bar) = triangle_lines(pivots)?;
    let ref_price = p(&pivots[pivots.len() - 1])?.abs();
    if !(lower.slope.abs() < upper.slope.abs() && upper.slope < 0.0) {
        return None;
    }
    let s_flat = flatness_score(lower.slope, ref_price, cfg.flatness_threshold_pct);
    let s_apex = apex_score(upper, lower, last_bar, cfg.apex_horizon_bars)?;
    if s_flat < cfg.flatness_min_score {
        return None;
    }
    Some(ShapeMatch {
        score: (s_flat + s_apex) / 2.0,
        invalidation: pivots
            .iter()
            .map(|p| p.price)
            .max()
            .unwrap_or(Decimal::ZERO),
        anchor_labels: vec!["P1", "P2", "P3", "P4"],
        variant: "bear",
    })
}

// ---------------------------------------------------------------------------
// Rectangle (P5.1)
// ---------------------------------------------------------------------------
//
// Flat upper band + flat lower band, built from 4 alternating pivots
// (HLHL or LHLH). Both trendlines must be near-horizontal (|slope|/ref
// per bar < rectangle_max_slope_pct) and the two highs / two lows must
// be approximately equal. Minimum duration guards against short noise
// ranges. Direction-neutral: the validator's breakout channels decide
// the side post-breach. Target formula (height × 1.0) lives in
// qtss-target-engine.
fn eval_rectangle(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    if pivots.len() != 4 {
        return None;
    }
    let (upper, lower, last_bar) = triangle_lines(pivots)?;
    let first_bar = pivots.iter().map(|p| p.bar_index).min().unwrap_or(last_bar);
    let span = last_bar.saturating_sub(first_bar);
    if span < cfg.rectangle_min_bars {
        return None;
    }
    let ref_price = p(&pivots[pivots.len() - 1])?.abs();
    // Both bands must be effectively flat.
    let s_flat_upper = flatness_score(upper.slope, ref_price, cfg.rectangle_max_slope_pct);
    let s_flat_lower = flatness_score(lower.slope, ref_price, cfg.rectangle_max_slope_pct);
    if s_flat_upper < cfg.flatness_min_score || s_flat_lower < cfg.flatness_min_score {
        return None;
    }
    // Collect highs / lows for equality scoring.
    let mut highs: Vec<f64> = Vec::new();
    let mut lows: Vec<f64> = Vec::new();
    for piv in pivots {
        let y = p(piv)?;
        match piv.kind {
            PivotKind::High => highs.push(y),
            PivotKind::Low => lows.push(y),
        }
    }
    if highs.len() != 2 || lows.len() != 2 {
        return None;
    }
    let upper_band = (highs[0] + highs[1]) / 2.0;
    let lower_band = (lows[0] + lows[1]) / 2.0;
    if upper_band <= lower_band {
        return None;
    }
    let s_eq_up = equality_score(highs[0], highs[1], cfg.equality_tolerance)?;
    let s_eq_lo = equality_score(lows[0], lows[1], cfg.equality_tolerance)?;
    Some(ShapeMatch {
        score: (s_flat_upper + s_flat_lower + s_eq_up + s_eq_lo) / 4.0,
        // Rectangle breaks either direction; pick the widest extreme as
        // structural guard (mirrors symmetrical_triangle).
        invalidation: pivots
            .iter()
            .map(|p| p.price)
            .max()
            .unwrap_or(Decimal::ZERO),
        anchor_labels: vec!["R1", "S1", "R2", "S2"],
        variant: "neutral",
    })
}

// ---------------------------------------------------------------------------
// Wedge (P5.3)
// ---------------------------------------------------------------------------
//
// Rising wedge  (bearish): upper.slope > 0, lower.slope > 0,
//                          lower.slope > upper.slope (converging upward).
// Falling wedge (bullish): upper.slope < 0, lower.slope < 0,
//                          |upper.slope| > |lower.slope| (converging
//                          downward).
//
// Convergence is verified via the shared `apex_score` so the apex must
// fall within `apex_horizon_bars` of the last pivot. Score = (apex +
// convergence_strength) / 2 where convergence_strength is the relative
// gap between the two slopes.

fn convergence_score(fast_slope_abs: f64, slow_slope_abs: f64) -> Option<f64> {
    if fast_slope_abs <= 0.0 || slow_slope_abs <= 0.0 {
        return None;
    }
    if fast_slope_abs <= slow_slope_abs {
        return None;
    }
    // Ratio in (1, ∞). Map to (0, 1) via 1 - 1/ratio.
    let ratio = fast_slope_abs / slow_slope_abs;
    Some(1.0 - 1.0 / ratio)
}

fn eval_rising_wedge(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    let (upper, lower, last_bar) = triangle_lines(pivots)?;
    if !(upper.slope > 0.0 && lower.slope > 0.0) {
        return None;
    }
    // Lower line rises faster than upper → converging upward.
    let s_conv = convergence_score(lower.slope, upper.slope)?;
    let s_apex = apex_score(upper, lower, last_bar, cfg.apex_horizon_bars)?;
    Some(ShapeMatch {
        score: (s_apex + s_conv) / 2.0,
        // Bearish reversal: invalidation = highest pivot (close above ⇒ pattern broken).
        invalidation: pivots
            .iter()
            .map(|p| p.price)
            .max()
            .unwrap_or(Decimal::ZERO),
        anchor_labels: vec!["W1", "W2", "W3", "W4"],
        variant: "bear",
    })
}

fn eval_falling_wedge(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    let (upper, lower, last_bar) = triangle_lines(pivots)?;
    if !(upper.slope < 0.0 && lower.slope < 0.0) {
        return None;
    }
    // Upper line falls faster than lower → converging downward.
    let s_conv = convergence_score(upper.slope.abs(), lower.slope.abs())?;
    let s_apex = apex_score(upper, lower, last_bar, cfg.apex_horizon_bars)?;
    Some(ShapeMatch {
        score: (s_apex + s_conv) / 2.0,
        // Bullish reversal: invalidation = lowest pivot.
        invalidation: pivots
            .iter()
            .map(|p| p.price)
            .min()
            .unwrap_or(Decimal::ZERO),
        anchor_labels: vec!["W1", "W2", "W3", "W4"],
        variant: "bull",
    })
}

// ---------------------------------------------------------------------------
// Price Channel (P5.4)
// ---------------------------------------------------------------------------
//
// Two near-parallel trendlines, both sloping the same way and steep
// enough to be considered trending (else the engine catches it as a
// rectangle). Direction: positive slope → ascending (bullish bias),
// negative → descending (bearish bias). Score blends parallelism with
// slope strength.

fn eval_channel_side(
    pivots: &[Pivot],
    cfg: &ClassicalConfig,
    expect_up: bool,
) -> Option<ShapeMatch> {
    let (upper, lower, _last_bar) = triangle_lines(pivots)?;
    let pole_sign: f64 = if expect_up { 1.0 } else { -1.0 };
    if upper.slope * pole_sign <= 0.0 || lower.slope * pole_sign <= 0.0 {
        return None;
    }
    // Duration gate.
    let first_bar = pivots.iter().map(|p| p.bar_index).min().unwrap_or(0);
    let last_bar = pivots.iter().map(|p| p.bar_index).max().unwrap_or(0);
    if last_bar.saturating_sub(first_bar) < cfg.channel_min_bars {
        return None;
    }
    // Trend strength gate (avoid masquerading rectangles).
    let ref_price = p(&pivots[pivots.len() - 1])?.abs();
    if ref_price <= 0.0 {
        return None;
    }
    let upper_pct = upper.slope.abs() / ref_price;
    let lower_pct = lower.slope.abs() / ref_price;
    if upper_pct < cfg.channel_min_slope_pct || lower_pct < cfg.channel_min_slope_pct {
        return None;
    }
    // Parallelism.
    let s_parallel = equality_score(
        upper.slope.abs(),
        lower.slope.abs(),
        cfg.channel_parallelism_tol,
    )?;
    // Slope-strength score: more above the floor → higher.
    let s_slope = (((upper_pct + lower_pct) / 2.0) / cfg.channel_min_slope_pct - 1.0)
        .clamp(0.0, 1.0);
    // Invalidation: ascending channel breaks DOWN below lowest pivot;
    // descending breaks UP above highest pivot.
    let invalidation = if expect_up {
        pivots
            .iter()
            .map(|p| p.price)
            .min()
            .unwrap_or(Decimal::ZERO)
    } else {
        pivots
            .iter()
            .map(|p| p.price)
            .max()
            .unwrap_or(Decimal::ZERO)
    };
    Some(ShapeMatch {
        score: (s_parallel + s_slope) / 2.0,
        invalidation,
        anchor_labels: vec!["C1", "C2", "C3", "C4"],
        variant: if expect_up { "bull" } else { "bear" },
    })
}

fn eval_ascending_channel(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    eval_channel_side(pivots, cfg, true)
}

fn eval_descending_channel(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    eval_channel_side(pivots, cfg, false)
}

// ---------------------------------------------------------------------------
// Diamond (P5.6)
// ---------------------------------------------------------------------------
//
// Diamond top  (bear): H1 L1 H2 L2 H3 L3 — broadening (H2>H1, L2<L1)
//                       then converging (H3<H2, L3>L2). The middle pair
//                       (H2,L2) defines the widest range.
// Diamond bottom (bull): mirror — L1 H1 L2 H2 L3 H3.
//
// Score = how cleanly the broadening + converging asymmetries hold,
// blended with width contraction strength.

fn eval_diamond_top(pivots: &[Pivot], _cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    if !require_kinds(
        pivots,
        &[
            PivotKind::High, PivotKind::Low, PivotKind::High,
            PivotKind::Low, PivotKind::High, PivotKind::Low,
        ],
    ) {
        return None;
    }
    let h1 = p(&pivots[0])?;
    let l1 = p(&pivots[1])?;
    let h2 = p(&pivots[2])?;
    let l2 = p(&pivots[3])?;
    let h3 = p(&pivots[4])?;
    let l3 = p(&pivots[5])?;
    // Broadening half: middle wider than first.
    if !(h2 > h1 && l2 < l1) {
        return None;
    }
    // Converging half: end narrower than middle.
    if !(h3 < h2 && l3 > l2) {
        return None;
    }
    let widest = h2 - l2;
    let initial = h1 - l1;
    let final_ = h3 - l3;
    if widest <= 0.0 || initial <= 0.0 || final_ <= 0.0 || widest <= initial.max(final_) {
        return None;
    }
    let s_broad = ((widest - initial) / widest).clamp(0.0, 1.0);
    let s_conv = ((widest - final_) / widest).clamp(0.0, 1.0);
    Some(ShapeMatch {
        score: (s_broad + s_conv) / 2.0,
        invalidation: pivots
            .iter()
            .map(|p| p.price)
            .max()
            .unwrap_or(Decimal::ZERO),
        anchor_labels: vec!["H1", "L1", "H2", "L2", "H3", "L3"],
        variant: "bear",
    })
}

fn eval_diamond_bottom(pivots: &[Pivot], _cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    if !require_kinds(
        pivots,
        &[
            PivotKind::Low, PivotKind::High, PivotKind::Low,
            PivotKind::High, PivotKind::Low, PivotKind::High,
        ],
    ) {
        return None;
    }
    let l1 = p(&pivots[0])?;
    let h1 = p(&pivots[1])?;
    let l2 = p(&pivots[2])?;
    let h2 = p(&pivots[3])?;
    let l3 = p(&pivots[4])?;
    let h3 = p(&pivots[5])?;
    if !(l2 < l1 && h2 > h1) {
        return None;
    }
    if !(l3 > l2 && h3 < h2) {
        return None;
    }
    let widest = h2 - l2;
    let initial = h1 - l1;
    let final_ = h3 - l3;
    if widest <= 0.0 || initial <= 0.0 || final_ <= 0.0 || widest <= initial.max(final_) {
        return None;
    }
    let s_broad = ((widest - initial) / widest).clamp(0.0, 1.0);
    let s_conv = ((widest - final_) / widest).clamp(0.0, 1.0);
    Some(ShapeMatch {
        score: (s_broad + s_conv) / 2.0,
        invalidation: pivots
            .iter()
            .map(|p| p.price)
            .min()
            .unwrap_or(Decimal::ZERO),
        anchor_labels: vec!["L1", "H1", "L2", "H2", "L3", "H3"],
        variant: "bull",
    })
}

// ---------------------------------------------------------------------------
// Bar-aware shape registry (P5.2)
// ---------------------------------------------------------------------------
//
// Flags and pennants need bar geometry — the flagpole is a fast price
// thrust measured in ATR multiples, which can't be seen from pivots
// alone. `ShapeSpecBars` keeps the same trait-like dispatch as
// `ShapeSpec` but hands the evaluator the trailing bar slice too
// (CLAUDE.md #1 — one extra entry in `SHAPES_WITH_BARS`, no central
// match).
pub struct ShapeSpecBars {
    pub name: &'static str,
    pub pivots_needed: usize,
    /// Minimum trailing bar count needed for flagpole / ATR calculation.
    pub bars_needed: usize,
    pub eval: fn(&[Pivot], &[Bar], &ClassicalConfig) -> Option<ShapeMatch>,
}

pub const SHAPES_WITH_BARS: &[ShapeSpecBars] = &[
    ShapeSpecBars {
        name: "bull_flag",
        pivots_needed: 4,
        bars_needed: 25,
        eval: eval_bull_flag,
    },
    ShapeSpecBars {
        name: "bear_flag",
        pivots_needed: 4,
        bars_needed: 25,
        eval: eval_bear_flag,
    },
    ShapeSpecBars {
        name: "pennant",
        pivots_needed: 4,
        bars_needed: 25,
        eval: eval_pennant,
    },
    // P5.5 — Cup & Handle (bull) + Inverse Cup & Handle (bear). 4 pivot:
    // rim_left, apex, rim_right, handle_extreme. Bar slice ile rim arası
    // parabolic R² yuvarlaklığı doğrulanır.
    ShapeSpecBars {
        name: "cup_and_handle",
        pivots_needed: 4,
        bars_needed: 35,
        eval: eval_cup_and_handle,
    },
    ShapeSpecBars {
        name: "inverse_cup_and_handle",
        pivots_needed: 4,
        bars_needed: 35,
        eval: eval_inverse_cup_and_handle,
    },
    // P5.5 — Rounding bottom (bull) / Rounding top (bear). 3 pivot:
    // rim_left, apex, rim_right. Handle yok.
    ShapeSpecBars {
        name: "rounding_bottom",
        pivots_needed: 3,
        bars_needed: 45,
        eval: eval_rounding_bottom,
    },
    ShapeSpecBars {
        name: "rounding_top",
        pivots_needed: 3,
        bars_needed: 45,
        eval: eval_rounding_top,
    },
];

// ---- parabolic fit (R²) helper for cup / rounding curvature ---------------

/// Least-squares parabolic fit y = a*x² + b*x + c on `ys` indexed by
/// x = 0..n. Returns R² in [0, 1] or None if degenerate.
fn parabolic_r2(ys: &[f64]) -> Option<f64> {
    let n = ys.len();
    if n < 5 {
        return None;
    }
    let nf = n as f64;
    let (mut sx, mut sx2, mut sx3, mut sx4) = (0.0, 0.0, 0.0, 0.0);
    let (mut sy, mut sxy, mut sx2y) = (0.0, 0.0, 0.0);
    for (i, &y) in ys.iter().enumerate() {
        let x = i as f64;
        let x2 = x * x;
        sx += x;
        sx2 += x2;
        sx3 += x2 * x;
        sx4 += x2 * x2;
        sy += y;
        sxy += x * y;
        sx2y += x2 * y;
    }
    // 3×3 normal equations:
    // | n   sx  sx2 | |c|   | sy  |
    // | sx  sx2 sx3 | |b| = | sxy |
    // | sx2 sx3 sx4 | |a|   | sx2y|
    let det = nf * (sx2 * sx4 - sx3 * sx3) - sx * (sx * sx4 - sx2 * sx3)
        + sx2 * (sx * sx3 - sx2 * sx2);
    if det.abs() < f64::EPSILON {
        return None;
    }
    let det_c = sy * (sx2 * sx4 - sx3 * sx3) - sx * (sxy * sx4 - sx2 * sx3 * sx2y / sx2.max(1e-12))
        + sx2 * (sxy * sx3 - sx2 * sx2y);
    let det_b = nf * (sxy * sx4 - sx3 * sx2y) - sy * (sx * sx4 - sx2 * sx3)
        + sx2 * (sx * sx2y - sx2 * sxy);
    let det_a = nf * (sx2 * sx2y - sx3 * sxy) - sx * (sx * sx2y - sx2 * sxy)
        + sy * (sx * sx3 - sx2 * sx2);
    let c = det_c / det;
    let b = det_b / det;
    let a = det_a / det;
    let mean_y = sy / nf;
    let (mut ss_res, mut ss_tot) = (0.0, 0.0);
    for (i, &y) in ys.iter().enumerate() {
        let x = i as f64;
        let yhat = a * x * x + b * x + c;
        ss_res += (y - yhat).powi(2);
        ss_tot += (y - mean_y).powi(2);
    }
    if ss_tot <= 0.0 {
        return None;
    }
    Some((1.0 - ss_res / ss_tot).clamp(0.0, 1.0))
}

/// Slice of bar closes between two bar indices, inclusive. Maps the
/// pivot bar_index space onto the bars slice (bars[i] ↔ index i, where
/// the slice starts at the orchestrator's window origin).
fn closes_between(bars: &[Bar], from_idx: u64, to_idx: u64) -> Option<Vec<f64>> {
    if to_idx <= from_idx {
        return None;
    }
    let last = bars.len().saturating_sub(1) as u64;
    if to_idx > last {
        return None;
    }
    Some(
        bars[from_idx as usize..=to_idx as usize]
            .iter()
            .map(bar_close)
            .collect(),
    )
}

// ---- Cup & Handle ----------------------------------------------------------

fn eval_cup_handle_side(
    pivots: &[Pivot],
    bars: &[Bar],
    cfg: &ClassicalConfig,
    bull: bool,
) -> Option<ShapeMatch> {
    if pivots.len() != 4 {
        return None;
    }
    // Bull: rim_left H, apex L, rim_right H, handle_low L.
    // Bear: rim_left L, apex H, rim_right L, handle_high H.
    let expected = if bull {
        [PivotKind::High, PivotKind::Low, PivotKind::High, PivotKind::Low]
    } else {
        [PivotKind::Low, PivotKind::High, PivotKind::Low, PivotKind::High]
    };
    if !require_kinds(pivots, &expected) {
        return None;
    }
    let rim_l = p(&pivots[0])?;
    let apex = p(&pivots[1])?;
    let rim_r = p(&pivots[2])?;
    let handle = p(&pivots[3])?;
    // Cup duration.
    let span = pivots[2].bar_index.saturating_sub(pivots[0].bar_index);
    if span < cfg.cup_min_bars {
        return None;
    }
    // Rim equality.
    let s_rim = equality_score(rim_l, rim_r, cfg.cup_rim_equality_tol)?;
    // Cup depth.
    let rim_avg = (rim_l + rim_r) / 2.0;
    let depth = if bull { rim_avg - apex } else { apex - rim_avg };
    if depth <= 0.0 {
        return None;
    }
    let depth_pct = depth / rim_avg.abs();
    if depth_pct < cfg.cup_min_depth_pct || depth_pct > cfg.cup_max_depth_pct {
        return None;
    }
    // Handle: opposite-direction shallow pullback after rim_right.
    let handle_depth = if bull { rim_r - handle } else { handle - rim_r };
    if handle_depth <= 0.0 {
        return None;
    }
    if handle_depth >= cfg.handle_max_depth_pct_of_cup * depth {
        return None;
    }
    // Curvature: parabolic R² over closes between rim_left and rim_right.
    let closes = closes_between(bars, pivots[0].bar_index, pivots[2].bar_index)?;
    let r2 = parabolic_r2(&closes)?;
    if r2 < cfg.cup_roundness_r2 {
        return None;
    }
    let s_round = ((r2 - cfg.cup_roundness_r2) / (1.0 - cfg.cup_roundness_r2).max(1e-9))
        .clamp(0.0, 1.0);
    let s_depth = {
        let mid = (cfg.cup_min_depth_pct + cfg.cup_max_depth_pct) / 2.0;
        let half = (cfg.cup_max_depth_pct - cfg.cup_min_depth_pct) / 2.0;
        1.0 - ((depth_pct - mid).abs() / half).clamp(0.0, 1.0)
    };
    // Invalidation: bull → handle low; bear → handle high.
    let invalidation = pivots[3].price;
    Some(ShapeMatch {
        score: (s_rim + s_round + s_depth) / 3.0,
        invalidation,
        anchor_labels: vec!["RimL", "Apex", "RimR", "Handle"],
        variant: if bull { "bull" } else { "bear" },
    })
}

fn eval_cup_and_handle(pivots: &[Pivot], bars: &[Bar], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    eval_cup_handle_side(pivots, bars, cfg, true)
}

fn eval_inverse_cup_and_handle(
    pivots: &[Pivot],
    bars: &[Bar],
    cfg: &ClassicalConfig,
) -> Option<ShapeMatch> {
    eval_cup_handle_side(pivots, bars, cfg, false)
}

// ---- Rounding (saucer) -----------------------------------------------------

fn eval_rounding_side(
    pivots: &[Pivot],
    bars: &[Bar],
    cfg: &ClassicalConfig,
    bull: bool,
) -> Option<ShapeMatch> {
    if pivots.len() != 3 {
        return None;
    }
    let expected = if bull {
        [PivotKind::High, PivotKind::Low, PivotKind::High]
    } else {
        [PivotKind::Low, PivotKind::High, PivotKind::Low]
    };
    if !require_kinds(pivots, &expected) {
        return None;
    }
    let rim_l = p(&pivots[0])?;
    let apex = p(&pivots[1])?;
    let rim_r = p(&pivots[2])?;
    let span = pivots[2].bar_index.saturating_sub(pivots[0].bar_index);
    if span < cfg.rounding_min_bars {
        return None;
    }
    // For bull rounding bottom apex must be a low BELOW both rims (sanity).
    let valid_geom = if bull { apex < rim_l && apex < rim_r } else { apex > rim_l && apex > rim_r };
    if !valid_geom {
        return None;
    }
    let s_rim = equality_score(rim_l, rim_r, cfg.cup_rim_equality_tol)?;
    let closes = closes_between(bars, pivots[0].bar_index, pivots[2].bar_index)?;
    let r2 = parabolic_r2(&closes)?;
    if r2 < cfg.rounding_roundness_r2 {
        return None;
    }
    let s_round = ((r2 - cfg.rounding_roundness_r2) / (1.0 - cfg.rounding_roundness_r2).max(1e-9))
        .clamp(0.0, 1.0);
    // Invalidation: bull → apex (sürekli aşağı kırılım pattern'i bozar);
    // bear → apex (yukarı kırılım).
    let invalidation = pivots[1].price;
    Some(ShapeMatch {
        score: (s_rim + s_round) / 2.0,
        invalidation,
        anchor_labels: vec!["RimL", "Apex", "RimR"],
        variant: if bull { "bull" } else { "bear" },
    })
}

fn eval_rounding_bottom(
    pivots: &[Pivot],
    bars: &[Bar],
    cfg: &ClassicalConfig,
) -> Option<ShapeMatch> {
    eval_rounding_side(pivots, bars, cfg, true)
}

fn eval_rounding_top(pivots: &[Pivot], bars: &[Bar], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    eval_rounding_side(pivots, bars, cfg, false)
}

// ---- bar helpers (ATR + flagpole detection) ------------------------------

fn bar_close(b: &Bar) -> f64 {
    b.close.to_f64().unwrap_or(0.0)
}

fn bar_high(b: &Bar) -> f64 {
    b.high.to_f64().unwrap_or(0.0)
}

fn bar_low(b: &Bar) -> f64 {
    b.low.to_f64().unwrap_or(0.0)
}

/// Wilder ATR across the window. Returns None if the window is too
/// short (< period + 1).
fn atr_window(bars: &[Bar], period: usize) -> Option<f64> {
    if bars.len() < period + 1 {
        return None;
    }
    let mut trs: Vec<f64> = Vec::with_capacity(bars.len() - 1);
    for i in 1..bars.len() {
        let h = bar_high(&bars[i]);
        let l = bar_low(&bars[i]);
        let pc = bar_close(&bars[i - 1]);
        let tr = (h - l).max((h - pc).abs()).max((l - pc).abs());
        trs.push(tr);
    }
    if trs.len() < period {
        return None;
    }
    let mut atr = trs[..period].iter().sum::<f64>() / period as f64;
    for &tr in &trs[period..] {
        atr = (atr * (period as f64 - 1.0) + tr) / period as f64;
    }
    if atr <= 0.0 {
        return None;
    }
    Some(atr)
}

/// Flagpole inspection for bars ending at (but not including) the first
/// flag pivot's bar_index. Returns (move, atr, direction_sign) where
/// direction_sign is +1 for up, -1 for down.
fn flagpole_measure(
    bars: &[Bar],
    lookback: usize,
    atr_period: usize,
) -> Option<(f64, f64, i8)> {
    // Bar slice is assumed to be aligned so bars[i].bar_index == first bar's
    // offset + i. We use the last `lookback + atr_period + 1` bars ending
    // at the bar just before the flag body. If the orchestrator feeds a
    // wider slice we still locate the flag-start by scanning from the end.
    if bars.is_empty() {
        return None;
    }
    // Find flag-start anchor as the last bar whose index is <= first flag
    // pivot. For simplicity we assume the bar slice ends at the "now" bar
    // and the flag body occupies its tail. We locate a cut `k` such that
    // bars[..k] ~ flagpole window and use its tail.
    //
    // Heuristic without bar_index: treat the *last `lookback` bars before
    // the minimum flag pivot* as the flagpole. Caller must pass a slice
    // whose length ≥ lookback + flag_span + atr_period + 1.
    if bars.len() < lookback + atr_period + 1 {
        return None;
    }
    // Flagpole window = bars[pole_start..pole_end], flag body after that.
    // Caller passes `bars` truncated so that the last bar is the flag-start
    // anchor; we take the last `lookback + atr_period + 1` bars for ATR
    // and `lookback` for the move measurement.
    let atr = atr_window(bars, atr_period)?;
    let pole_start = bars.len().saturating_sub(lookback + 1);
    let start_close = bar_close(&bars[pole_start]);
    let end_close = bar_close(&bars[bars.len() - 1]);
    let mv = end_close - start_close;
    let sign: i8 = if mv > 0.0 { 1 } else if mv < 0.0 { -1 } else { return None };
    Some((mv.abs(), atr, sign))
}

/// Core flag evaluator (direction-parametrised).
fn eval_flag_side(
    pivots: &[Pivot],
    bars: &[Bar],
    cfg: &ClassicalConfig,
    expect_up: bool,
) -> Option<ShapeMatch> {
    if pivots.len() != 4 {
        return None;
    }
    // Flag body = parallel-ish counter-trend channel across 4 alternating
    // pivots. Bull flag retraces DOWN after an up-pole, so flag lines
    // have negative slope; bear flag mirrors.
    let (upper, lower, _last_bar) = triangle_lines(pivots)?;
    // Both lines should slope the same direction, opposite to flagpole.
    let pole_sign: f64 = if expect_up { 1.0 } else { -1.0 };
    if upper.slope * pole_sign >= 0.0 || lower.slope * pole_sign >= 0.0 {
        return None;
    }
    // Parallelism: |upper.slope - lower.slope| / avg small.
    let s_parallel = equality_score(
        upper.slope.abs(),
        lower.slope.abs(),
        cfg.flag_parallelism_tol,
    )?;
    // Flagpole: look at bars BEFORE the earliest flag pivot. We slice the
    // incoming bar window to the bars up to the first flag pivot index.
    let first_flag_bar = pivots.iter().map(|p| p.bar_index).min().unwrap_or(0);
    // Map pivot bar_index onto the bar slice. The bar slice is the
    // chronological last-N bars fed to the runner; the pivot tree uses
    // the same indexing. We assume bars[i].bar_index alignment is NOT
    // available as a field — so we use the *tail offset* instead: locate
    // the position by subtracting from the slice's implied last bar idx.
    //
    // Simpler contract: the caller ensures bars.len() covers from some
    // historical bar up to (at least) the first flag pivot. We find the
    // cut index by pivot.bar_index relative to bars' last index (which we
    // take as the pivot tree's max bar_index ≈ bars.len()-1 under the
    // pipeline's current alignment).
    let last_bar_idx_in_slice = bars.len().saturating_sub(1) as u64;
    // bars are ordered oldest..newest, indexed 0..=last_bar_idx_in_slice.
    // If first_flag_bar > last_bar_idx_in_slice we can't measure.
    if first_flag_bar > last_bar_idx_in_slice {
        return None;
    }
    // Cut = bars up to (exclusive) first_flag_bar.
    let cut = first_flag_bar as usize;
    if cut == 0 {
        return None;
    }
    let pole_slice = &bars[..cut];
    let (pole_move, _atr, pole_sign_measured) = flagpole_measure(
        pole_slice,
        cfg.flag_pole_max_bars as usize,
        cfg.flag_atr_period as usize,
    )?;
    // Direction must agree with expected side.
    if (pole_sign_measured as f64) * pole_sign <= 0.0 {
        return None;
    }
    // ATR-scaled strength check.
    let atr = atr_window(pole_slice, cfg.flag_atr_period as usize)?;
    if pole_move < cfg.flag_pole_min_move_atr * atr {
        return None;
    }
    // Flag height vs flagpole magnitude.
    let mut highs: Vec<f64> = Vec::new();
    let mut lows: Vec<f64> = Vec::new();
    for piv in pivots {
        let y = p(piv)?;
        match piv.kind {
            PivotKind::High => highs.push(y),
            PivotKind::Low => lows.push(y),
        }
    }
    if highs.len() != 2 || lows.len() != 2 {
        return None;
    }
    let flag_height = (highs.iter().cloned().fold(f64::MIN, f64::max)
        - lows.iter().cloned().fold(f64::MAX, f64::min))
    .abs();
    if flag_height >= cfg.flag_max_retrace_pct * pole_move {
        return None;
    }
    // Scoring: parallelism + ATR strength component (capped 1.0) + retrace
    // tightness.
    let s_pole = ((pole_move / (cfg.flag_pole_min_move_atr * atr)) - 1.0)
        .clamp(0.0, 1.0);
    let s_retrace = 1.0 - (flag_height / (cfg.flag_max_retrace_pct * pole_move)).clamp(0.0, 1.0);
    // Invalidation: bull flag breaks DOWN below lower band; bear flag
    // breaks UP above upper band. Take the relevant extreme pivot.
    let invalidation = if expect_up {
        pivots
            .iter()
            .map(|p| p.price)
            .min()
            .unwrap_or(Decimal::ZERO)
    } else {
        pivots
            .iter()
            .map(|p| p.price)
            .max()
            .unwrap_or(Decimal::ZERO)
    };
    Some(ShapeMatch {
        score: (s_parallel + s_pole + s_retrace) / 3.0,
        invalidation,
        anchor_labels: vec!["F1", "F2", "F3", "F4"],
        variant: if expect_up { "bull" } else { "bear" },
    })
}

fn eval_bull_flag(pivots: &[Pivot], bars: &[Bar], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    eval_flag_side(pivots, bars, cfg, true)
}

fn eval_bear_flag(pivots: &[Pivot], bars: &[Bar], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    eval_flag_side(pivots, bars, cfg, false)
}

fn eval_pennant(pivots: &[Pivot], bars: &[Bar], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    // Pennant = flagpole followed by a small symmetrical triangle.
    if pivots.len() != 4 {
        return None;
    }
    let (upper, lower, last_bar) = triangle_lines(pivots)?;
    // Symmetrical convergence: upper slopes down, lower slopes up.
    if !(upper.slope < 0.0 && lower.slope > 0.0) {
        return None;
    }
    let s_apex = apex_score(upper, lower, last_bar, cfg.apex_horizon_bars)?;
    let s_sym = equality_score(upper.slope.abs(), lower.slope, cfg.triangle_symmetry_tol)?;
    // Flagpole gate.
    let first_flag_bar = pivots.iter().map(|p| p.bar_index).min().unwrap_or(0);
    let last_bar_idx_in_slice = bars.len().saturating_sub(1) as u64;
    if first_flag_bar > last_bar_idx_in_slice {
        return None;
    }
    let cut = first_flag_bar as usize;
    if cut == 0 {
        return None;
    }
    let pole_slice = &bars[..cut];
    let (pole_move, atr, pole_sign) = flagpole_measure(
        pole_slice,
        cfg.flag_pole_max_bars as usize,
        cfg.flag_atr_period as usize,
    )?;
    if pole_move < cfg.flag_pole_min_move_atr * atr {
        return None;
    }
    // Pennant body max height.
    let mut highs_y: Vec<f64> = Vec::new();
    let mut lows_y: Vec<f64> = Vec::new();
    for piv in pivots {
        let y = p(piv)?;
        match piv.kind {
            PivotKind::High => highs_y.push(y),
            PivotKind::Low => lows_y.push(y),
        }
    }
    if highs_y.len() != 2 || lows_y.len() != 2 {
        return None;
    }
    let body_height = (highs_y.iter().cloned().fold(f64::MIN, f64::max)
        - lows_y.iter().cloned().fold(f64::MAX, f64::min))
    .abs();
    if body_height >= cfg.pennant_max_height_pct_of_pole * pole_move {
        return None;
    }
    let s_pole = ((pole_move / (cfg.flag_pole_min_move_atr * atr)) - 1.0).clamp(0.0, 1.0);
    let variant = if pole_sign > 0 { "bull" } else { "bear" };
    let invalidation = if pole_sign > 0 {
        pivots
            .iter()
            .map(|p| p.price)
            .min()
            .unwrap_or(Decimal::ZERO)
    } else {
        pivots
            .iter()
            .map(|p| p.price)
            .max()
            .unwrap_or(Decimal::ZERO)
    };
    Some(ShapeMatch {
        score: (s_apex + s_sym + s_pole) / 3.0,
        invalidation,
        anchor_labels: vec!["P1", "P2", "P3", "P4"],
        variant,
    })
}

fn eval_symmetrical_triangle(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    let (upper, lower, last_bar) = triangle_lines(pivots)?;
    if !(upper.slope < 0.0 && lower.slope > 0.0) {
        return None;
    }
    let s_apex = apex_score(upper, lower, last_bar, cfg.apex_horizon_bars)?;
    // Symmetry: how close |upper.slope| and lower.slope are.
    let s_sym = equality_score(upper.slope.abs(), lower.slope, cfg.triangle_symmetry_tol)?;
    Some(ShapeMatch {
        score: (s_apex + s_sym) / 2.0,
        // direction unknown until breakout — use widest extreme as guard
        invalidation: pivots
            .iter()
            .map(|p| p.price)
            .max()
            .unwrap_or(Decimal::ZERO),
        anchor_labels: vec!["P1", "P2", "P3", "P4"],
        variant: "neutral",
    })
}
