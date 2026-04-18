//! Forward-projection engine.
//!
//! Once a formation is detected, this module produces the *expected*
//! next anchors using canonical Fibonacci relationships. The output is
//! a `Vec<PivotRef>` whose entries reuse the same `PivotRef` shape as
//! realized anchors but carry projected `bar_index` (one bar past the
//! last realized pivot, monotone increasing) and a `label` prefixed with
//! `~` to mark the anchor as a projection.
//!
//! Each formation has its own per-shape projector — registered via a
//! dispatch table keyed by the subkind prefix (CLAUDE.md #1: no
//! scattered match arms). Adding a new projection = one entry in the
//! `PROJECTORS` table.
//!
//! Conventions per Frost & Prechter:
//!
//!   * **Impulse 1-2-3-4-5** (after a 5-wave count completes):
//!       projected next move is an A-B-C corrective:
//!         A = 0.382 × impulse range, in the opposing direction
//!         B = 0.5  × A, retracing toward the impulse end
//!         C = 1.0  × A, extending past A
//!   * **Zigzag A-B-C** (correction completed): projected next move is
//!       a continuation of the original (pre-correction) trend, with
//!       a target of 1.272 × C-leg from the C end.
//!   * **Triangle A-B-C-D-E** (consolidation completed): "thrust" =
//!       the widest point of the triangle, projected from E in the
//!       direction opposing the last leg.
//!   * **Diagonals** (leading or ending): leading → expect a strong
//!       impulse continuation in the wedge direction; ending → expect
//!       sharp reversal to wedge start.
//!   * **Flat A-B-C** (correction completed): same as zigzag — trend
//!       resumption — but with a smaller continuation target since flats
//!       imply weaker corrective pressure.

use qtss_domain::v2::detection::PivotRef;
use qtss_domain::v2::pivot::PivotLevel;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

/// Single projector function. Takes the realized anchors plus the
/// pivot level (carried through to projected anchors) and returns the
/// projected anchors. Empty `Vec` means the projector decided no
/// projection is meaningful for this case.
type Projector = fn(&[PivotRef], PivotLevel) -> Vec<PivotRef>;

/// Dispatch table — first matching subkind prefix wins. Order matters
/// only for prefix collisions (e.g. `impulse_truncated_5` would match
/// `impulse` first, so the more specific entry is listed first).
const PROJECTORS: &[(&str, Projector)] = &[
    ("impulse_truncated_5", project_truncated_fifth),
    ("impulse_w1_extended", project_extended_impulse),
    ("impulse_w3_extended", project_extended_impulse),
    ("impulse_w5_extended", project_extended_impulse),
    ("impulse_5", project_impulse_5),
    ("zigzag_abc", project_zigzag),
    ("flat_regular", project_flat),
    ("flat_expanded", project_flat),
    ("flat_running", project_flat),
    ("triangle_contracting", project_triangle),
    ("triangle_expanding", project_triangle),
    ("triangle_barrier", project_triangle),
    ("triangle_running", project_triangle),
    ("leading_diagonal_5_3_5", project_leading_diagonal),
    ("ending_diagonal_3_3_3", project_ending_diagonal),
];

/// Public entry point used by every formation detector right before it
/// constructs its `Detection`. Returns an empty Vec when no projector
/// matches the subkind — the detector simply ships realized anchors
/// only in that case.
pub fn project(subkind: &str, realized: &[PivotRef], level: PivotLevel) -> Vec<PivotRef> {
    for (prefix, proj) in PROJECTORS {
        if subkind.starts_with(prefix) {
            return proj(realized, level);
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------
// Helpers shared by every projector
// ---------------------------------------------------------------------

/// Build a projected `PivotRef` at `bar_offset` bars past the last
/// realized pivot, with `~`-prefixed label. The price is computed by
/// the caller in f64 then converted to Decimal.
fn projected(
    last_idx: u64,
    bar_offset: u64,
    price: f64,
    level: PivotLevel,
    label: &str,
) -> PivotRef {
    PivotRef {
        bar_index: last_idx + bar_offset,
        price: Decimal::from_f64(price).unwrap_or_default(),
        level,
        label: Some(format!("~{label}")),
    }
}

fn last_anchor_index(realized: &[PivotRef]) -> u64 {
    realized.last().map(|a| a.bar_index).unwrap_or(0)
}

/// Average bar spacing between realized anchors — used to space the
/// projected anchors at a roughly natural cadence rather than dropping
/// them all on the very next bar.
fn avg_spacing(realized: &[PivotRef]) -> u64 {
    if realized.len() < 2 {
        return 1;
    }
    let span = realized.last().unwrap().bar_index - realized.first().unwrap().bar_index;
    let n = realized.len() as u64 - 1;
    (span / n).max(1)
}

fn to_f64(d: Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

// ---------------------------------------------------------------------
// Per-formation projectors
// ---------------------------------------------------------------------

/// After a complete 1-2-3-4-5 impulse, project the corrective A-B-C.
/// Range = p5 - p0 in raw signed terms; corrective targets are fractions
/// of that range, oriented opposite to the impulse direction.
fn project_impulse_5(realized: &[PivotRef], level: PivotLevel) -> Vec<PivotRef> {
    if realized.len() < 6 {
        return Vec::new();
    }
    let p0 = to_f64(realized[0].price);
    let p5 = to_f64(realized[5].price);
    let range = p5 - p0;
    if range == 0.0 {
        return Vec::new();
    }
    // Corrective leg moves opposite to (p5 - p0).
    let dir = -range.signum();
    let span = range.abs();
    let a = p5 + dir * 0.382 * span;
    let b = a - dir * 0.5 * (span * 0.382);
    let c = p5 + dir * 0.618 * span;

    let last = last_anchor_index(realized);
    let step = avg_spacing(realized);
    vec![
        projected(last, step, a, level, "A"),
        projected(last, 2 * step, b, level, "B"),
        projected(last, 3 * step, c, level, "C"),
    ]
}

/// Truncated 5th implies an *immediate* sharp reversal — project a
/// single deep retrace to ~0.618 of the entire impulse range.
fn project_truncated_fifth(realized: &[PivotRef], level: PivotLevel) -> Vec<PivotRef> {
    if realized.len() < 6 {
        return Vec::new();
    }
    let p0 = to_f64(realized[0].price);
    let p5 = to_f64(realized[5].price);
    let range = p5 - p0;
    if range == 0.0 {
        return Vec::new();
    }
    let target = p5 - range * 0.618;
    vec![projected(
        last_anchor_index(realized),
        avg_spacing(realized) * 2,
        target,
        level,
        "rev",
    )]
}

/// Extended impulse projects a stronger A-B-C correction (the extended
/// wave drives a larger pullback).
fn project_extended_impulse(realized: &[PivotRef], level: PivotLevel) -> Vec<PivotRef> {
    if realized.len() < 6 {
        return Vec::new();
    }
    let p0 = to_f64(realized[0].price);
    let p5 = to_f64(realized[5].price);
    let range = p5 - p0;
    if range == 0.0 {
        return Vec::new();
    }
    let dir = -range.signum();
    let span = range.abs();
    // Stronger pullback: 0.5 / 0.786 of impulse range.
    let a = p5 + dir * 0.5 * span;
    let c = p5 + dir * 0.786 * span;
    let last = last_anchor_index(realized);
    let step = avg_spacing(realized);
    vec![
        projected(last, step, a, level, "A"),
        projected(last, 3 * step, c, level, "C"),
    ]
}

/// After a zigzag completes, project trend resumption — a continuation
/// of the pre-correction direction, target = 1.272 × C-leg past C end.
fn project_zigzag(realized: &[PivotRef], level: PivotLevel) -> Vec<PivotRef> {
    if realized.len() < 4 {
        return Vec::new();
    }
    let c_end = to_f64(realized[3].price);
    let c_start = to_f64(realized[2].price);
    let c_leg = c_end - c_start;
    if c_leg == 0.0 {
        return Vec::new();
    }
    // Trend resumption opposes the C-leg direction.
    let target = c_end - c_leg * 1.272;
    vec![projected(
        last_anchor_index(realized),
        avg_spacing(realized) * 2,
        target,
        level,
        "trend",
    )]
}

/// Flats imply weaker corrective pressure — smaller continuation
/// target than zigzag.
fn project_flat(realized: &[PivotRef], level: PivotLevel) -> Vec<PivotRef> {
    if realized.len() < 4 {
        return Vec::new();
    }
    let c_end = to_f64(realized[3].price);
    let a_start = to_f64(realized[0].price);
    let leg = c_end - a_start;
    if leg == 0.0 {
        return Vec::new();
    }
    let target = c_end - leg * 0.618;
    vec![projected(
        last_anchor_index(realized),
        avg_spacing(realized) * 2,
        target,
        level,
        "trend",
    )]
}

/// Triangle "thrust" projection: from E (last realized anchor),
/// project a move equal to the *widest* part of the triangle in the
/// direction opposite to the final leg.
fn project_triangle(realized: &[PivotRef], level: PivotLevel) -> Vec<PivotRef> {
    if realized.len() < 6 {
        return Vec::new();
    }
    let prices: Vec<f64> = realized.iter().map(|a| to_f64(a.price)).collect();
    let max = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min = prices.iter().cloned().fold(f64::INFINITY, f64::min);
    let width = max - min;
    if width == 0.0 {
        return Vec::new();
    }
    let last_leg = prices[5] - prices[4];
    if last_leg == 0.0 {
        return Vec::new();
    }
    let dir = -last_leg.signum();
    let target = prices[5] + dir * width;
    vec![projected(
        last_anchor_index(realized),
        avg_spacing(realized) * 2,
        target,
        level,
        "thrust",
    )]
}

/// Leading diagonal → strong impulse continuation in wedge direction.
fn project_leading_diagonal(realized: &[PivotRef], level: PivotLevel) -> Vec<PivotRef> {
    if realized.len() < 6 {
        return Vec::new();
    }
    let p0 = to_f64(realized[0].price);
    let p5 = to_f64(realized[5].price);
    let range = p5 - p0;
    if range == 0.0 {
        return Vec::new();
    }
    // Continuation: project another 1.0× wedge range past p5.
    let target = p5 + range;
    vec![projected(
        last_anchor_index(realized),
        avg_spacing(realized) * 3,
        target,
        level,
        "cont",
    )]
}

/// Ending diagonal → sharp reversal back to wedge start.
fn project_ending_diagonal(realized: &[PivotRef], level: PivotLevel) -> Vec<PivotRef> {
    if realized.len() < 6 {
        return Vec::new();
    }
    let p0 = to_f64(realized[0].price);
    let target = p0; // full retrace of the wedge
    vec![projected(
        last_anchor_index(realized),
        avg_spacing(realized) * 3,
        target,
        level,
        "rev",
    )]
}
