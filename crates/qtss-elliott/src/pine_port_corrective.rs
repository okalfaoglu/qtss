//! LuxAlgo Pine port — corrective-pattern refinement (Flat + Triangle).
//!
//! Runs **after** the Pine port's state machine has emitted an ABC or
//! the 5-pivot window that could be a triangle, and tags the result
//! with its Frost & Prechter sub-type. This module does **not** touch
//! the motive / ABC / fib / break-box detection — it only annotates.
//!
//! Pattern rules (copied from `crate::flat` and `crate::triangle` so
//! both the FormationDetector world and the Pine port agree on the
//! same classification thresholds — a future refactor can fold them
//! into a single source):
//!
//! * **Flat** — ABC where `|B|/|A| ≥ 0.90` (if < 0.90 it stays a
//!   zigzag). Sub-type dispatch table `FLAT_SUBTYPES` classifies as
//!   regular / expanded / running based on `(b_ratio, c_ratio)`.
//! * **Triangle** — five alternating pivots A/B/C/D/E whose two
//!   same-side ratios (top line = A→C→E, bottom line = B→D) dispatch
//!   through `TRIANGLE_SUBTYPES` as contracting / expanding / barrier.
//!
//! Keeping this module small and pure (no I/O, no state) satisfies the
//! CLAUDE.md rule against scattered if/else — every classification is a
//! dispatch-table look-up.

use crate::luxalgo_pine_port::{AbcPattern, PivotPoint, TrianglePattern};

/// Per-classifier entry — `(label, min_b_ratio, min_c_ratio, max_c_ratio)`.
/// First row to match wins, so order matters: running before expanded
/// before regular, because running's `max_c < 1.0` is tighter than
/// expanded's overlapping range but they share the `min_b = 1.05` floor.
/// Mirror of `crate::flat::SUBTYPES`.
const FLAT_SUBTYPES: &[(&str, f64, f64, f64)] = &[
    ("running", 1.05, 0.0, 1.0),
    ("expanded", 1.05, 1.05, 2.0),
    ("regular", 0.90, 0.85, 1.15),
];

/// Minimum |B|/|A| for a flat. Below this, the ABC is a plain zigzag.
const FLAT_MIN_B_RATIO: f64 = 0.90;

type TrianglePredicate = fn(top_ratio: f64, bot_ratio: f64) -> bool;
const TRIANGLE_SUBTYPES: &[(&str, TrianglePredicate)] = &[
    ("contracting", is_contracting),
    ("expanding", is_expanding),
    ("barrier", is_barrier),
];
const BARRIER_FLAT_TOL: f64 = 0.05;

fn is_contracting(top_ratio: f64, bot_ratio: f64) -> bool {
    top_ratio < 1.0 && bot_ratio < 1.0
}
fn is_expanding(top_ratio: f64, bot_ratio: f64) -> bool {
    top_ratio > 1.0 && bot_ratio > 1.0
}
fn is_barrier(top_ratio: f64, bot_ratio: f64) -> bool {
    let top_flat = (top_ratio - 1.0).abs() < BARRIER_FLAT_TOL;
    let bot_flat = (bot_ratio - 1.0).abs() < BARRIER_FLAT_TOL;
    (top_flat && bot_ratio < 1.0) || (bot_flat && top_ratio < 1.0)
}

/// Classify an ABC's sub-type. Returns the matching `FLAT_SUBTYPES`
/// label (with a `"flat_"` prefix) when the B retracement is deep
/// enough; otherwise falls back to `"zigzag"` so the caller always
/// gets a non-`None` subkind.
pub fn classify_abc_subkind(abc: &AbcPattern) -> &'static str {
    let p0 = abc.anchors[0].price;
    let p1 = abc.anchors[1].price;
    let p2 = abc.anchors[2].price;
    let p3 = abc.anchors[3].price;

    let a_leg = p1 - p0;
    let b_leg = p2 - p1;
    let c_leg = p3 - p2;

    let a_abs = a_leg.abs();
    if a_abs == 0.0 || c_leg == 0.0 {
        return "zigzag";
    }
    // Strict alternation rule — A and C same sign, B opposite.
    if a_leg.signum() != c_leg.signum() {
        return "zigzag";
    }
    if b_leg.signum() == a_leg.signum() {
        return "zigzag";
    }

    let b_ratio = b_leg.abs() / a_abs;
    if b_ratio < FLAT_MIN_B_RATIO {
        return "zigzag";
    }
    let c_ratio = c_leg.abs() / a_abs;

    for &(label, min_b, min_c, max_c) in FLAT_SUBTYPES {
        if b_ratio >= min_b && c_ratio >= min_c && c_ratio <= max_c {
            return match label {
                "running" => "flat_running",
                "expanded" => "flat_expanded",
                "regular" => "flat_regular",
                _ => "zigzag",
            };
        }
    }
    "zigzag"
}

/// Detect a triangle from the most recent 6 alternating pivots. Input
/// slice may contain arbitrarily many; only the tail is inspected.
///
/// Returns `None` when the shape fails alternation, has a zero leg, or
/// doesn't match any `TRIANGLE_SUBTYPES` predicate.
pub fn detect_triangle(pivots: &[PivotPoint]) -> Option<TrianglePattern> {
    if pivots.len() < 6 {
        return None;
    }
    let tail = &pivots[pivots.len() - 6..];

    // Strict alternation: directions must be ±1 ±1 ±1 ±1 ±1 ±1 with
    // opposite sign on each step.
    for i in 1..6 {
        if tail[i].direction.signum() == tail[i - 1].direction.signum() {
            return None;
        }
    }

    let p: [f64; 6] = [
        tail[0].price, tail[1].price, tail[2].price,
        tail[3].price, tail[4].price, tail[5].price,
    ];

    let a_leg = p[1] - p[0];
    if a_leg == 0.0 {
        return None;
    }

    // For a triangle that starts bearish (first_leg < 0, A is a down
    // leg), the "top" line connects highs at indices 0, 2, 4 and the
    // "bottom" line connects lows at indices 1, 3, 5. For a bullish
    // start the roles flip; we always pick indices so that the earlier
    // same-parity pair defines the first segment.
    let top_idx: [usize; 3] = if a_leg < 0.0 { [0, 2, 4] } else { [1, 3, 5] };
    let bot_idx: [usize; 3] = if a_leg < 0.0 { [1, 3, 5] } else { [0, 2, 4] };

    // Ratio of the second top-side leg to the first (e.g. |C-E| / |A-C|).
    let top_leg1 = (p[top_idx[1]] - p[top_idx[0]]).abs();
    let top_leg2 = (p[top_idx[2]] - p[top_idx[1]]).abs();
    let bot_leg1 = (p[bot_idx[1]] - p[bot_idx[0]]).abs();
    let bot_leg2 = (p[bot_idx[2]] - p[bot_idx[1]]).abs();
    if top_leg1 == 0.0 || bot_leg1 == 0.0 {
        return None;
    }
    let top_ratio = top_leg2 / top_leg1;
    let bot_ratio = bot_leg2 / bot_leg1;

    for &(label, pred) in TRIANGLE_SUBTYPES {
        if pred(top_ratio, bot_ratio) {
            let direction: i8 = if a_leg < 0.0 { -1 } else { 1 };
            let subkind = match label {
                "contracting" => "triangle_contracting",
                "expanding" => "triangle_expanding",
                "barrier" => "triangle_barrier",
                _ => return None,
            };
            return Some(TrianglePattern {
                direction,
                subkind: subkind.to_string(),
                anchors: [
                    tail[0].clone(), tail[1].clone(), tail[2].clone(),
                    tail[3].clone(), tail[4].clone(), tail[5].clone(),
                ],
                invalidated: false,
            });
        }
    }
    None
}
