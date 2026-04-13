//! Shared helpers used by every formation detector.
//!
//! Each formation lives in its own file (CLAUDE.md #1: dispatch table over
//! scattered match arms). Anything that more than one formation needs —
//! direction normalization, anchor labeling, structural-score helpers —
//! is hoisted here so the per-formation files stay focused on rules.

use qtss_domain::v2::detection::PivotRef;
use qtss_domain::v2::pivot::{Pivot, PivotKind, PivotLevel};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

/// Compass for the leg a formation describes. Used by every formation as
/// the first thing it decides — bullish patterns start with a low pivot,
/// bearish ones with a high pivot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Bullish,
    Bearish,
}

impl Direction {
    pub fn from_first(kind: PivotKind) -> Self {
        match kind {
            PivotKind::Low => Direction::Bullish,
            PivotKind::High => Direction::Bearish,
        }
    }

    /// Sign for normalizing prices to a "bullish-positive" frame: bearish
    /// formations negate prices so a single rule set covers both sides.
    pub fn sign(self) -> Decimal {
        match self {
            Direction::Bullish => Decimal::ONE,
            Direction::Bearish => Decimal::NEGATIVE_ONE,
        }
    }

    /// Suffix used in PatternKind subkinds — `..._bull` / `..._bear`.
    pub fn suffix(self) -> &'static str {
        match self {
            Direction::Bullish => "bull",
            Direction::Bearish => "bear",
        }
    }
}

/// Normalize a slice of pivots into a `Vec<f64>` where prices increase
/// in the formation's "natural" direction. Bearish formations get their
/// prices negated so structural rules can be written once.
pub fn normalize(pivots: &[Pivot], dir: Direction) -> Vec<f64> {
    let sign = dir.sign();
    pivots
        .iter()
        .map(|p| (p.price * sign).to_f64().unwrap_or(0.0))
        .collect()
}

/// Verify the pivots strictly alternate kind. Most formations need this
/// before any rule check makes sense.
pub fn alternation_ok(pivots: &[Pivot]) -> bool {
    pivots
        .windows(2)
        .all(|w| w[0].kind != w[1].kind)
}

/// Build labelled `PivotRef`s from raw pivots + a static label list.
/// Pass labels like `["0","1","2","3","4","5"]` for impulses or
/// `["0","A","B","C"]` for zigzags.
pub fn label_anchors(
    pivots: &[Pivot],
    level: PivotLevel,
    labels: &[&'static str],
) -> Vec<PivotRef> {
    pivots
        .iter()
        .zip(labels.iter())
        .map(|(p, l)| PivotRef {
            bar_index: p.bar_index,
            price: p.price,
            level,
            label: Some((*l).to_string()),
        })
        .collect()
}

/// Gaussian-style proximity score for an observed ratio against the
/// nearest of `refs`. Identical kernel width to `fibs::proximity_score`
/// — kept here as a private helper so formations don't have to import
/// the legacy module.
/// Gaussian scoring: how close `observed` is to one of the `refs`.
/// `width` controls tolerance — larger = more forgiving.
/// Professional EW tools use ±5-8% tolerance bands (Frost & Prechter
/// treat ratios as guidelines, not rules). A width of 0.12 gives:
///   - ±5% deviation → score ~0.92
///   - ±10% deviation → score ~0.71
///   - ±15% deviation → score ~0.47
/// Previous width=0.05 was too strict — ±5% deviation scored only 0.61.
pub fn nearest_fib_score(observed: f64, refs: &[f64]) -> f64 {
    let nearest = refs
        .iter()
        .map(|r| (observed - r).abs())
        .fold(f64::INFINITY, f64::min);
    let width = 0.12;
    (-(nearest * nearest) / (2.0 * width * width)).exp()
}

/// Average of any number of sub-scores (skipping NaN). Each formation
/// composes its structural score from a small basket — keeping the
/// aggregation in one helper means tweaking the weighting policy is a
/// one-line change.
pub fn mean_score(parts: &[f64]) -> f64 {
    let mut sum = 0.0;
    let mut n = 0.0;
    for p in parts {
        if p.is_finite() {
            sum += p;
            n += 1.0;
        }
    }
    if n == 0.0 {
        0.0
    } else {
        sum / n
    }
}
