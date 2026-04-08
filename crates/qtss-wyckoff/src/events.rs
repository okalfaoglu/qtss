//! Wyckoff event catalog.
//!
//! Each entry is an [`EventSpec`]: a name and an `eval` function pointer
//! that inspects the trailing pivots + computed [`TradingRange`] and
//! returns a [`EventMatch`] when it fires. The detector walks every spec
//! through the same loop and keeps the highest-scoring match — adding a
//! new event (Sign-of-Strength, Last-Point-of-Support, …) is one slice
//! entry, no central match arm to edit (CLAUDE.md rule #1).

use crate::config::WyckoffConfig;
use crate::range::{average_volume, TradingRange};
use qtss_domain::v2::pivot::{Pivot, PivotKind};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct EventMatch {
    pub score: f64,
    pub invalidation: Decimal,
    pub variant: &'static str,
    /// Anchor labels for the trailing pivots (oldest..newest).
    pub anchor_labels: Vec<&'static str>,
}

pub struct EventSpec {
    pub name: &'static str,
    pub eval: fn(&[Pivot], &WyckoffConfig) -> Option<EventMatch>,
}

pub const EVENTS: &[EventSpec] = &[
    EventSpec {
        name: "trading_range",
        eval: eval_trading_range,
    },
    EventSpec {
        name: "spring",
        eval: eval_spring,
    },
    EventSpec {
        name: "upthrust",
        eval: eval_upthrust,
    },
];

// ---------------------------------------------------------------------------
// Trading range
// ---------------------------------------------------------------------------

fn eval_trading_range(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < cfg.min_range_pivots {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let tightness = range.edge_tightness(pivots, cfg.range_edge_tolerance)?;
    if tightness < 0.4 {
        return None;
    }

    // Variant: where did the climactic-volume pivot land?
    // High side  -> distribution (BC)
    // Low side   -> accumulation (SC)
    let variant = climactic_variant(pivots, &range, cfg).unwrap_or("neutral");
    let labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();

    Some(EventMatch {
        score: tightness,
        // a clean break of the *opposite* side from the climax invalidates
        // the wyckoff thesis; pick the support as a conservative anchor.
        invalidation: Decimal::try_from(range.support).ok().unwrap_or(Decimal::ZERO),
        variant,
        anchor_labels: labels,
    })
}

fn label_for(idx: usize) -> &'static str {
    const LABELS: &[&str] = &[
        "P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8", "P9", "P10", "P11", "P12",
    ];
    LABELS.get(idx).copied().unwrap_or("Pn")
}

fn climactic_variant(
    pivots: &[Pivot],
    range: &TradingRange,
    cfg: &WyckoffConfig,
) -> Option<&'static str> {
    let avg = average_volume(pivots)?.to_f64()?;
    if avg <= 0.0 {
        return None;
    }
    let threshold = avg * cfg.climax_volume_mult;
    // Find the most climactic pivot, then look at which edge of the range
    // it sits closest to.
    let mut best: Option<(&Pivot, f64)> = None;
    for p in pivots {
        let v = p.volume_at_pivot.to_f64()?;
        if v >= threshold && best.map(|(_, bv)| v > bv).unwrap_or(true) {
            best = Some((p, v));
        }
    }
    let (climax, _) = best?;
    let price = climax.price.to_f64()?;
    let d_top = (range.resistance - price).abs();
    let d_bot = (price - range.support).abs();
    let v = match climax.kind {
        PivotKind::Low if d_bot <= d_top => "accumulation",
        PivotKind::High if d_top <= d_bot => "distribution",
        _ => return None,
    };
    Some(v)
}

// ---------------------------------------------------------------------------
// Spring (bullish false-break of support followed by re-entry)
// ---------------------------------------------------------------------------

fn eval_spring(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < cfg.min_range_pivots + 1 {
        return None;
    }
    let context = &pivots[..pivots.len() - 1];
    let candidate = pivots.last()?;
    if candidate.kind != PivotKind::Low {
        return None;
    }
    let range = TradingRange::from_pivots(context)?;
    let price = candidate.price.to_f64()?;
    if price >= range.support {
        return None;
    }
    let penetration = (range.support - price) / range.height.max(1e-9);
    if penetration < cfg.min_penetration || penetration > cfg.max_penetration {
        return None;
    }
    // Score: best when penetration sits in the middle of the allowed band.
    let center = (cfg.min_penetration + cfg.max_penetration) / 2.0;
    let half = (cfg.max_penetration - cfg.min_penetration) / 2.0;
    let z = (penetration - center) / half.max(1e-9);
    let score = (-(z * z) / 2.0).exp();
    let labels: Vec<&'static str> = (0..context.len())
        .map(label_for)
        .chain(std::iter::once("Spring"))
        .collect();
    // A spring that fails (price closes below the spring low itself)
    // invalidates the bullish thesis.
    Some(EventMatch {
        score,
        invalidation: candidate.price,
        variant: "bull",
        anchor_labels: labels,
    })
}

// ---------------------------------------------------------------------------
// Upthrust (bearish false-break of resistance)
// ---------------------------------------------------------------------------

fn eval_upthrust(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < cfg.min_range_pivots + 1 {
        return None;
    }
    let context = &pivots[..pivots.len() - 1];
    let candidate = pivots.last()?;
    if candidate.kind != PivotKind::High {
        return None;
    }
    let range = TradingRange::from_pivots(context)?;
    let price = candidate.price.to_f64()?;
    if price <= range.resistance {
        return None;
    }
    let penetration = (price - range.resistance) / range.height.max(1e-9);
    if penetration < cfg.min_penetration || penetration > cfg.max_penetration {
        return None;
    }
    let center = (cfg.min_penetration + cfg.max_penetration) / 2.0;
    let half = (cfg.max_penetration - cfg.min_penetration) / 2.0;
    let z = (penetration - center) / half.max(1e-9);
    let score = (-(z * z) / 2.0).exp();
    let labels: Vec<&'static str> = (0..context.len())
        .map(label_for)
        .chain(std::iter::once("UTAD"))
        .collect();
    Some(EventMatch {
        score,
        invalidation: candidate.price,
        variant: "bear",
        anchor_labels: labels,
    })
}
