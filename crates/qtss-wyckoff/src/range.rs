//! Wyckoff range detection — groups consecutive events into a
//! schematic box (Accumulation or Distribution) with bounding price
//! levels and the current phase.
//!
//! Algorithm:
//!   1. Walk events chronologically. A range OPENS only when a
//!      CLIMAX event fires (SC for accumulation, BC for distribution).
//!      Wyckoff theory: nothing before the climax counts toward the
//!      range — capitulation/distribution is what frames it.
//!   2. While a range is OPEN, fold subsequent events into it:
//!      Accumulation:
//!        - range_LOW: SC, ST, Spring, LPS, Test, PS
//!        - range_HIGH: AR, SOS, BU
//!      Distribution:
//!        - range_HIGH: BC, UTAD
//!        - range_LOW: AR, ST, LPS, SOW
//!      AR is bias-dependent (post-SC HIGH = top of accum range,
//!      post-BC LOW = bottom of dist range).
//!   3. The range CLOSES when the OPPOSITE climax fires (BC opens
//!      the new distribution range; SC opens the new accum range).
//!      The phase tracker's `WyckoffPhase::E` flag also marks the
//!      box `completed` so renderers can dim it.

use crate::event::{WyckoffEvent, WyckoffEventKind};
use crate::phase::{WyckoffBias, WyckoffPhase, WyckoffPhaseTracker};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WyckoffRange {
    pub bias: WyckoffBias,
    pub phase: WyckoffPhase,
    pub start_bar: usize,
    pub end_bar: usize,
    pub range_high: f64,
    pub range_low: f64,
    /// Indices of events (into the input slice) that contributed to
    /// this range — useful for renderers showing event count.
    pub event_indices: Vec<usize>,
    /// `true` when a Phase-E breakout has confirmed the schematic;
    /// `false` while still building.
    pub completed: bool,
}

/// Group `events` (must be in chronological order) into one or more
/// `WyckoffRange` boxes. Empty when no climax (SC or BC) is present.
pub fn detect_ranges(events: &[WyckoffEvent]) -> Vec<WyckoffRange> {
    let mut out: Vec<WyckoffRange> = Vec::new();
    if events.is_empty() {
        return out;
    }

    // Sort indices chronologically by bar_index.
    let mut idx: Vec<usize> = (0..events.len()).collect();
    idx.sort_by_key(|&i| events[i].bar_index);

    let mut current: Option<(WyckoffRange, WyckoffPhaseTracker)> = None;

    for &i in &idx {
        let ev = &events[i];

        // Climax events open or close ranges.
        let opens_accum = ev.kind == WyckoffEventKind::Sc;
        let opens_dist = ev.kind == WyckoffEventKind::Bc;

        if opens_accum || opens_dist {
            // Close any existing range first.
            if let Some((mut r, _)) = current.take() {
                r.completed = true;
                if r.range_high.is_finite()
                    && r.range_low.is_finite()
                    && r.range_high > r.range_low
                {
                    out.push(r);
                }
            }
            let bias = if opens_accum {
                WyckoffBias::Accumulation
            } else {
                WyckoffBias::Distribution
            };
            // Seed the range with this climax as both initial high
            // and low — the SC low / BC high anchors the schematic
            // boundary so a range with no other events still draws
            // a degenerate (but technically valid) box.
            let mut tracker = WyckoffPhaseTracker::new();
            tracker.feed(ev);
            current = Some((
                WyckoffRange {
                    bias,
                    phase: tracker.phase(),
                    start_bar: ev.bar_index,
                    end_bar: ev.bar_index,
                    range_high: ev.reference_price,
                    range_low: ev.reference_price,
                    event_indices: vec![i],
                    completed: false,
                },
                tracker,
            ));
            continue;
        }

        // Non-climax event: fold into the open range if any.
        let Some((r, tracker)) = current.as_mut() else {
            continue;
        };
        tracker.feed(ev);
        r.phase = tracker.phase();
        r.end_bar = r.end_bar.max(ev.bar_index);
        r.event_indices.push(i);

        match (r.bias, ev.kind) {
            // Accumulation lows.
            (
                WyckoffBias::Accumulation,
                WyckoffEventKind::St
                | WyckoffEventKind::Spring
                | WyckoffEventKind::Lps
                | WyckoffEventKind::Test
                | WyckoffEventKind::Ps,
            ) => {
                if ev.reference_price < r.range_low {
                    r.range_low = ev.reference_price;
                }
            }
            // Accumulation highs.
            (
                WyckoffBias::Accumulation,
                WyckoffEventKind::Ar
                | WyckoffEventKind::Sos
                | WyckoffEventKind::Bu,
            ) => {
                if ev.reference_price > r.range_high {
                    r.range_high = ev.reference_price;
                }
            }
            // Distribution highs.
            (
                WyckoffBias::Distribution,
                WyckoffEventKind::Utad,
            ) => {
                if ev.reference_price > r.range_high {
                    r.range_high = ev.reference_price;
                }
            }
            // Distribution lows.
            (
                WyckoffBias::Distribution,
                WyckoffEventKind::Sow
                | WyckoffEventKind::St
                | WyckoffEventKind::Lps
                | WyckoffEventKind::Ar,
            ) => {
                if ev.reference_price < r.range_low {
                    r.range_low = ev.reference_price;
                }
            }
            _ => {}
        }
    }

    if let Some((mut r, _)) = current.take() {
        // Mark completed when the tracker reached Phase E earlier.
        if r.phase == WyckoffPhase::E {
            r.completed = true;
        }
        if r.range_high.is_finite()
            && r.range_low.is_finite()
            && r.range_high > r.range_low
        {
            out.push(r);
        }
    }
    out
}
