//! Sub-wave decomposition.
//!
//! Per Elliott Wave Principle, every wave at a given fractal degree
//! breaks down into smaller waves of one degree below. The
//! `decompose` helper takes the realized anchors a formation produced
//! and the full `PivotTree`, then walks the *next-finer* pivot level
//! and groups its pivots by which higher-degree segment they fall in.
//!
//! Output shape: `Vec<Vec<PivotRef>>` with length `realized.len() - 1`.
//! Each inner vec holds the lower-degree pivots strictly between the
//! two anchors that bound that segment (exclusive on both ends — the
//! endpoints are the higher-degree pivots themselves and would be
//! redundant). When the formation already runs on the finest level
//! (`L0`) the function returns an empty outer vec.
//!
//! Conventions:
//!
//!   * Bar indices on lower-degree pivots are *strictly between* the
//!     two enclosing higher-degree bar indices.
//!   * Sub-wave PivotRefs carry an empty label by default — the chart
//!     decides how to render them (typically as fainter polylines).
//!   * No alternation / rule check is applied here — we just expose
//!     the raw points so the chart can draw them honestly.

use qtss_domain::v2::detection::PivotRef;
use qtss_domain::v2::pivot::{PivotLevel, PivotTree};

/// Pick the next-finer pivot level relative to `formation_level`. The
/// finest level is `L0`, so a formation already on `L0` has no further
/// decomposition. Returns `None` in that case.
fn next_finer(level: PivotLevel) -> Option<PivotLevel> {
    match level {
        PivotLevel::L0 => None,
        PivotLevel::L1 => Some(PivotLevel::L0),
        PivotLevel::L2 => Some(PivotLevel::L1),
        PivotLevel::L3 => Some(PivotLevel::L2),
        PivotLevel::L4 => Some(PivotLevel::L3),
    }
}

/// Decompose every consecutive pair of realized anchors into the
/// lower-degree pivots that fall between them.
pub fn decompose(
    tree: &PivotTree,
    realized: &[PivotRef],
    formation_level: PivotLevel,
) -> Vec<Vec<PivotRef>> {
    if realized.len() < 2 {
        return Vec::new();
    }
    let Some(finer) = next_finer(formation_level) else {
        return Vec::new();
    };
    let lower = tree.at_level(finer);
    if lower.is_empty() {
        return Vec::new();
    }

    realized
        .windows(2)
        .map(|w| {
            let (lo, hi) = (w[0].bar_index.min(w[1].bar_index), w[0].bar_index.max(w[1].bar_index));
            lower
                .iter()
                .filter(|p| p.bar_index > lo && p.bar_index < hi)
                .map(|p| PivotRef {
                    bar_index: p.bar_index,
                    price: p.price,
                    level: finer,
                    label: None,
                })
                .collect()
        })
        .collect()
}
