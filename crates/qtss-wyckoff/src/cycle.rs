//! Wyckoff macro market cycle — the four-phase rotation
//! (Accumulation → Markup → Distribution → Markdown) that frames the
//! schematic ranges from `range.rs`.
//!
//! `range.rs` describes ONE schematic box (a single accumulation OR a
//! single distribution); `cycle.rs` describes the WHOLE rotation
//! between consecutive ranges, including the trend legs that connect
//! them. Phase E breakout from an accumulation begins a Markup leg
//! that lasts until the next BC opens a Distribution; Phase E
//! breakdown from distribution begins a Markdown leg until the next
//! SC opens the new accumulation.
//!
//! Detection input: the same chronologically-sorted event slice the
//! range detector consumes. A simple state machine walks events and
//! emits a contiguous sequence of `WyckoffCycle` segments covering
//! every bar from the first climax onward — segments tile the price
//! tape with no gaps.
//!
//! State transitions (`current_phase` field):
//!   None    → Accumulation       on SC
//!   None    → Distribution       on BC
//!   Accumulation → Markup        on Bu / Sos (Phase E breakout)
//!                              OR on next BC  (skip-ahead — schematic
//!                                              never confirmed Phase E
//!                                              but the new climax
//!                                              proves the markup ran)
//!   Markup       → Distribution  on BC
//!   Distribution → Markdown      on Sow (Phase E breakdown)
//!                              OR on next SC
//!   Markdown     → Accumulation  on SC
//!
//! Boundaries: each segment's `start_bar`/`end_bar` are inclusive bar
//! indices into the input bar slice; `end_bar` is the bar index of
//! the climax (or breakout) that closes the segment, OR the LAST bar
//! of the input slice when the segment is still open at the tape
//! head.

use crate::event::{WyckoffEvent, WyckoffEventKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffCyclePhase {
    /// Sideways range after a downtrend — smart money absorbing supply.
    Accumulation,
    /// Trend leg up between an accumulation breakout and the next BC.
    Markup,
    /// Sideways range at the top — smart money distributing.
    Distribution,
    /// Trend leg down between a distribution breakdown and the next SC.
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WyckoffCycle {
    pub phase: WyckoffCyclePhase,
    /// Inclusive bar index where the segment opens.
    pub start_bar: usize,
    /// Inclusive bar index where the segment closes (= the climax /
    /// breakout that ends it, or the last fed bar when still open).
    pub end_bar: usize,
    /// Reference price at `start_bar` (climax / breakout price).
    pub start_price: f64,
    /// Reference price at `end_bar` (climax / breakout price, or last
    /// observed price for the open segment — caller fills in if it
    /// wants the live close).
    pub end_price: f64,
    /// `true` once the next phase opens; `false` while still active.
    pub completed: bool,
}

/// Walk `events` (chronological, by bar_index) and produce a
/// contiguous tiling of cycle segments. The final segment is left
/// `completed = false` and its `end_bar` is the tape head — callers
/// can update `end_bar` / `end_price` as new bars arrive.
///
/// `tape_end_bar` is the last bar index of the input price tape (used
/// to close the trailing open segment); `tape_end_price` likewise.
pub fn detect_cycles(
    events: &[WyckoffEvent],
    tape_end_bar: usize,
    tape_end_price: f64,
) -> Vec<WyckoffCycle> {
    let mut out: Vec<WyckoffCycle> = Vec::new();
    if events.is_empty() {
        return out;
    }

    // Sort indices chronologically.
    let mut idx: Vec<usize> = (0..events.len()).collect();
    idx.sort_by_key(|&i| events[i].bar_index);

    let mut current: Option<WyckoffCycle> = None;

    let close_into = |out: &mut Vec<WyckoffCycle>,
                      mut cur: WyckoffCycle,
                      end_bar: usize,
                      end_price: f64| {
        cur.end_bar = end_bar.max(cur.start_bar);
        cur.end_price = end_price;
        cur.completed = true;
        out.push(cur);
    };

    for &i in &idx {
        let ev = &events[i];
        match ev.kind {
            // Accumulation opens / Markdown closes.
            WyckoffEventKind::Sc => {
                if let Some(cur) = current.take() {
                    close_into(&mut out, cur, ev.bar_index, ev.reference_price);
                }
                current = Some(WyckoffCycle {
                    phase: WyckoffCyclePhase::Accumulation,
                    start_bar: ev.bar_index,
                    end_bar: ev.bar_index,
                    start_price: ev.reference_price,
                    end_price: ev.reference_price,
                    completed: false,
                });
            }
            // Distribution opens / Markup closes.
            WyckoffEventKind::Bc => {
                if let Some(cur) = current.take() {
                    close_into(&mut out, cur, ev.bar_index, ev.reference_price);
                }
                current = Some(WyckoffCycle {
                    phase: WyckoffCyclePhase::Distribution,
                    start_bar: ev.bar_index,
                    end_bar: ev.bar_index,
                    start_price: ev.reference_price,
                    end_price: ev.reference_price,
                    completed: false,
                });
            }
            // Phase E breakout from accumulation begins Markup.
            WyckoffEventKind::Bu | WyckoffEventKind::Sos => {
                if let Some(cur) = current.as_ref() {
                    if cur.phase == WyckoffCyclePhase::Accumulation {
                        let opened_at = ev.bar_index;
                        let opened_price = ev.reference_price;
                        if let Some(prev) = current.take() {
                            close_into(&mut out, prev, opened_at, opened_price);
                        }
                        current = Some(WyckoffCycle {
                            phase: WyckoffCyclePhase::Markup,
                            start_bar: opened_at,
                            end_bar: opened_at,
                            start_price: opened_price,
                            end_price: opened_price,
                            completed: false,
                        });
                    }
                }
            }
            // Phase E breakdown from distribution begins Markdown.
            WyckoffEventKind::Sow => {
                if let Some(cur) = current.as_ref() {
                    if cur.phase == WyckoffCyclePhase::Distribution {
                        let opened_at = ev.bar_index;
                        let opened_price = ev.reference_price;
                        if let Some(prev) = current.take() {
                            close_into(&mut out, prev, opened_at, opened_price);
                        }
                        current = Some(WyckoffCycle {
                            phase: WyckoffCyclePhase::Markdown,
                            start_bar: opened_at,
                            end_bar: opened_at,
                            start_price: opened_price,
                            end_price: opened_price,
                            completed: false,
                        });
                    }
                }
            }
            // Other events advance end_bar (so the open segment grows
            // with the tape) but don't change phase.
            _ => {}
        }
        if let Some(cur) = current.as_mut() {
            if ev.bar_index > cur.end_bar {
                cur.end_bar = ev.bar_index;
                cur.end_price = ev.reference_price;
            }
        }
    }

    if let Some(mut cur) = current.take() {
        cur.end_bar = tape_end_bar.max(cur.end_bar);
        cur.end_price = tape_end_price;
        out.push(cur);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: WyckoffEventKind, bar: usize, price: f64) -> WyckoffEvent {
        WyckoffEvent {
            kind,
            variant: "bull",
            score: 1.0,
            bar_index: bar,
            reference_price: price,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: String::new(),
        }
    }

    #[test]
    fn full_rotation_produces_four_phases() {
        let events = vec![
            ev(WyckoffEventKind::Sc, 10, 100.0),  // Accum opens
            ev(WyckoffEventKind::Bu, 30, 110.0),  // Markup opens
            ev(WyckoffEventKind::Bc, 60, 140.0),  // Dist opens
            ev(WyckoffEventKind::Sow, 80, 130.0), // Markdown opens
            ev(WyckoffEventKind::Sc, 110, 95.0),  // Next accum
        ];
        let cycles = detect_cycles(&events, 200, 100.0);
        assert_eq!(cycles.len(), 5);
        assert_eq!(cycles[0].phase, WyckoffCyclePhase::Accumulation);
        assert_eq!(cycles[1].phase, WyckoffCyclePhase::Markup);
        assert_eq!(cycles[2].phase, WyckoffCyclePhase::Distribution);
        assert_eq!(cycles[3].phase, WyckoffCyclePhase::Markdown);
        assert_eq!(cycles[4].phase, WyckoffCyclePhase::Accumulation);
        assert!(cycles[0].completed);
        assert!(!cycles[4].completed);
    }

    #[test]
    fn empty_input_returns_empty_output() {
        let cycles = detect_cycles(&[], 100, 50.0);
        assert!(cycles.is_empty());
    }

    #[test]
    fn missing_bu_skips_to_next_climax() {
        // Accumulation never confirmed Phase E, but a BC arrives — the
        // markup is implicit (price clearly ran from accum to dist).
        let events = vec![
            ev(WyckoffEventKind::Sc, 10, 100.0),
            ev(WyckoffEventKind::Bc, 60, 140.0),
        ];
        let cycles = detect_cycles(&events, 100, 130.0);
        assert_eq!(cycles.len(), 2);
        assert_eq!(cycles[0].phase, WyckoffCyclePhase::Accumulation);
        assert_eq!(cycles[1].phase, WyckoffCyclePhase::Distribution);
    }
}
