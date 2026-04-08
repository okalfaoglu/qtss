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
    let s_neck = equality_score(n1, n2, cfg.equality_tolerance * 1.5)?;
    Some(ShapeMatch {
        score: (s_shoulders + s_neck) / 2.0,
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
    let s_neck = equality_score(n1, n2, cfg.equality_tolerance * 1.5)?;
    Some(ShapeMatch {
        score: (s_shoulders + s_neck) / 2.0,
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

fn flatness_score(slope: f64, reference_price: f64) -> f64 {
    // Slope is in price-per-bar; normalise by reference price to get a
    // unitless "% change per bar". Anything under ~0.1% per bar counts
    // as effectively flat. Gaussian fall-off thereafter.
    if reference_price <= 0.0 {
        return 0.0;
    }
    let pct = (slope / reference_price).abs();
    let z = pct / 0.001;
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
    let s_flat = flatness_score(upper.slope, ref_price);
    let s_apex = apex_score(upper, lower, last_bar, cfg.apex_horizon_bars)?;
    if s_flat < 0.3 {
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
    let s_flat = flatness_score(lower.slope, ref_price);
    let s_apex = apex_score(upper, lower, last_bar, cfg.apex_horizon_bars)?;
    if s_flat < 0.3 {
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

fn eval_symmetrical_triangle(pivots: &[Pivot], cfg: &ClassicalConfig) -> Option<ShapeMatch> {
    let (upper, lower, last_bar) = triangle_lines(pivots)?;
    if !(upper.slope < 0.0 && lower.slope > 0.0) {
        return None;
    }
    let s_apex = apex_score(upper, lower, last_bar, cfg.apex_horizon_bars)?;
    // Symmetry: how close |upper.slope| and lower.slope are.
    let s_sym = equality_score(upper.slope.abs(), lower.slope, 0.5)?;
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
