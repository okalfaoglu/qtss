//! Wyckoff macro market cycle — the four-phase rotation
//! (Accumulation → Markup → Distribution → Markdown) that frames the
//! schematic ranges from `range.rs`.
//!
//! Two complementary detection paths feed the same `WyckoffCycle`
//! struct, distinguished by `source`:
//!
//! 1. **Event-driven** (`detect_cycles`, `detect_cycles_for_slot`):
//!    walk Wyckoff events (SC/BC/Bu/Sos/Sow). Low latency,
//!    volume-validated. Sparse — leaves gaps when no climax fires.
//!
//! 2. **Elliott-anchored** (`detect_cycles_from_elliott`): map per-slot
//!    motive (5-wave) + abc (corrective) detections to phases via
//!    Pruden's canonical mapping (Bullish motive ⇒ Markup; Bearish ABC
//!    ⇒ Markdown; transitions ⇒ Distribution / Accumulation). Fully
//!    contiguous tilesheet, predictive (forming W4 already implies a
//!    Markup tile that runs to the projected W5).
//!
//! 3. **Hybrid confluence** (`merge_cycles_with_confluence`): pair
//!    event-driven and elliott-anchored tiles that overlap on the
//!    same phase by ≥ `min_overlap_ratio`; emit a single
//!    `Source::Confluent` tile spanning the union. Highest-confidence
//!    signal — used downstream by Major-Dip composite scoring.
//!
//! See `docs/ELLIOTT_WYCKOFF_INTEGRATION.md` §VII for the theoretical
//! mapping (Pruden 2007, Wyckoff Analytics canonical course) and
//! edge-case treatment (truncated W5, extended W3, running flat /
//! triangle, W-X-Y combinations).

use crate::event::{WyckoffEvent, WyckoffEventKind};
use qtss_domain::v2::bar::Bar;
use rust_decimal::prelude::ToPrimitive;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffCycleSource {
    /// Derived from Wyckoff climax/breakout events
    /// (SC/BC/Bu/Sos/Sow). Volume-validated, low latency.
    Event,
    /// Derived from Elliott structural segments (motive + abc) via
    /// Pruden's canonical mapping. Continuous, predictive.
    Elliott,
    /// Both sources agree on phase + time window — highest confidence.
    Confluent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WyckoffCycle {
    pub phase: WyckoffCyclePhase,
    pub source: WyckoffCycleSource,
    /// Inclusive bar index where the segment opens.
    pub start_bar: usize,
    /// Inclusive bar index where the segment closes (= the climax /
    /// breakout that ends it, or the last fed bar when still open).
    pub end_bar: usize,
    /// Reference price at `start_bar`.
    pub start_price: f64,
    /// Reference price at `end_bar`.
    pub end_price: f64,
    /// Highest HIGH across bars in `[start_bar, end_bar]`.
    pub phase_high: f64,
    /// Lowest LOW across bars in `[start_bar, end_bar]`.
    pub phase_low: f64,
    /// `true` once the next phase opens; `false` while still active.
    pub completed: bool,
    /// Detection ID of the originating Elliott row (when source = Elliott
    /// or Confluent). Lets downstream queries trace back to W1..W5/A..C
    /// anchors.
    pub source_pattern_id: Option<String>,
}

/// Elliott structural segment used as input by
/// `detect_cycles_from_elliott`. Caller (engine writer) builds the
/// vector by querying `detections WHERE pattern_family IN
/// ('motive','abc') AND slot = N`.
#[derive(Debug, Clone, PartialEq)]
pub struct ElliottSegment {
    pub kind: ElliottSegmentKind,
    pub bullish: bool,
    pub start_bar: usize,
    pub end_bar: usize,
    pub start_price: f64,
    pub end_price: f64,
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElliottSegmentKind {
    /// 5-wave impulse (W0..W5). Bullish = uptrend Markup; bearish =
    /// downtrend Markdown leg.
    Motive,
    /// 3-wave correction (X0, A, B, C). Bearish (after up motive) =
    /// Markdown; bullish (counter-trend bounce) = Accumulation tile.
    Abc,
}

fn bar_high(b: &Bar) -> f64 {
    b.high.to_f64().unwrap_or(0.0)
}
fn bar_low(b: &Bar) -> f64 {
    b.low.to_f64().unwrap_or(0.0)
}

fn compute_bounds(
    bars: &[Bar],
    start: usize,
    end: usize,
    reference: f64,
) -> (f64, f64) {
    if bars.is_empty() {
        return (reference, reference);
    }
    let last = bars.len() - 1;
    let s = start.min(last);
    let e = end.min(last).max(s);
    let mut hi = f64::NEG_INFINITY;
    let mut lo = f64::INFINITY;
    for i in s..=e {
        let h = bar_high(&bars[i]);
        let l = bar_low(&bars[i]);
        if h.is_finite() && h > hi {
            hi = h;
        }
        if l.is_finite() && l < lo {
            lo = l;
        }
    }
    if !hi.is_finite() || !lo.is_finite() {
        (reference, reference)
    } else {
        (hi, lo)
    }
}

// ── Event-driven detection ────────────────────────────────────────

/// Build a contiguous tiling of cycle segments from Wyckoff `events`.
/// Tiles emitted carry `source = WyckoffCycleSource::Event`.
pub fn detect_cycles(
    events: &[WyckoffEvent],
    bars: &[Bar],
    tape_end_bar: usize,
    tape_end_price: f64,
) -> Vec<WyckoffCycle> {
    let mut out: Vec<WyckoffCycle> = Vec::new();
    if events.is_empty() {
        return out;
    }

    let mut idx: Vec<usize> = (0..events.len()).collect();
    idx.sort_by_key(|&i| events[i].bar_index);

    let mut current: Option<WyckoffCycle> = None;

    for &i in &idx {
        let ev = &events[i];
        match ev.kind {
            WyckoffEventKind::Sc => {
                if let Some(prev) = current.take() {
                    out.push(close_segment(prev, ev.bar_index, ev.reference_price, bars));
                }
                current = Some(open_segment(
                    WyckoffCyclePhase::Accumulation,
                    WyckoffCycleSource::Event,
                    ev.bar_index,
                    ev.reference_price,
                    None,
                ));
            }
            WyckoffEventKind::Bc => {
                if let Some(prev) = current.take() {
                    out.push(close_segment(prev, ev.bar_index, ev.reference_price, bars));
                }
                current = Some(open_segment(
                    WyckoffCyclePhase::Distribution,
                    WyckoffCycleSource::Event,
                    ev.bar_index,
                    ev.reference_price,
                    None,
                ));
            }
            WyckoffEventKind::Bu | WyckoffEventKind::Sos => {
                if let Some(prev) = current.as_ref() {
                    if prev.phase == WyckoffCyclePhase::Accumulation {
                        let opened_at = ev.bar_index;
                        let opened_price = ev.reference_price;
                        if let Some(prev) = current.take() {
                            out.push(close_segment(prev, opened_at, opened_price, bars));
                        }
                        current = Some(open_segment(
                            WyckoffCyclePhase::Markup,
                            WyckoffCycleSource::Event,
                            opened_at,
                            opened_price,
                            None,
                        ));
                    }
                }
            }
            WyckoffEventKind::Sow => {
                if let Some(prev) = current.as_ref() {
                    if prev.phase == WyckoffCyclePhase::Distribution {
                        let opened_at = ev.bar_index;
                        let opened_price = ev.reference_price;
                        if let Some(prev) = current.take() {
                            out.push(close_segment(prev, opened_at, opened_price, bars));
                        }
                        current = Some(open_segment(
                            WyckoffCyclePhase::Markdown,
                            WyckoffCycleSource::Event,
                            opened_at,
                            opened_price,
                            None,
                        ));
                    }
                }
            }
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
        let (hi, lo) =
            compute_bounds(bars, cur.start_bar, cur.end_bar, cur.start_price);
        cur.phase_high = hi;
        cur.phase_low = lo;
        out.push(cur);
    }

    out
}

/// Slot-aware variant: filters `events` by `min_score` before running
/// the state machine. Higher slots = stricter score gate.
pub fn detect_cycles_for_slot(
    events: &[WyckoffEvent],
    bars: &[Bar],
    min_score: f32,
    tape_end_bar: usize,
    tape_end_price: f64,
) -> Vec<WyckoffCycle> {
    let filtered: Vec<WyckoffEvent> = events
        .iter()
        .filter(|e| (e.score as f32) >= min_score)
        .cloned()
        .collect();
    detect_cycles(&filtered, bars, tape_end_bar, tape_end_price)
}

// ── Elliott-anchored detection ────────────────────────────────────

/// Build cycle tiles from chronologically-sorted Elliott `segments`
/// per Pruden's canonical mapping (see `ELLIOTT_WYCKOFF_INTEGRATION.md`
/// §VII.2). Tiles carry `source = WyckoffCycleSource::Elliott` and
/// `source_pattern_id = segment.source_id`.
///
/// Mapping rules:
///   * Bullish motive (5-up)   → Markup       [start_bar, end_bar]
///   * Bearish ABC (3-down)    → Markdown     [start_bar, end_bar]
///   * Bearish motive (5-down) → Markdown     [start_bar, end_bar]
///   * Bullish ABC (3-up)      → Accumulation [start_bar, end_bar]
///                               (counter-trend bounce or W2/W4 dip)
///
/// Gaps between consecutive segments are filled with transition
/// tiles: a gap after Markup/Markdown becomes a Distribution/
/// Accumulation respectively (the "topping" / "basing" zone).
pub fn detect_cycles_from_elliott(
    segments: &[ElliottSegment],
    bars: &[Bar],
    tape_end_bar: usize,
    tape_end_price: f64,
) -> Vec<WyckoffCycle> {
    let mut out: Vec<WyckoffCycle> = Vec::new();
    if segments.is_empty() {
        return out;
    }

    let mut sorted: Vec<&ElliottSegment> = segments.iter().collect();
    sorted.sort_by_key(|s| s.start_bar);

    let mut prev_end: Option<(usize, f64, WyckoffCyclePhase)> = None;
    let gap_min_bars: usize = 2;

    // LEADING fill — Pruden's mapping says every bull motive launches
    // from an Accumulation base (and every bear ABC starts inside a
    // Distribution top). Before the first segment, the tape was in
    // the phase that PRECEDES the first segment's phase. Emit that
    // as a context tile from bar 0 to the first segment's start so
    // the user sees Accumulation BEFORE the Markup leg, not just an
    // empty pre-motive tape.
    if let Some(first_seg) = sorted.first() {
        if first_seg.start_bar > gap_min_bars {
            let first_phase = phase_for_segment(first_seg);
            let leading_phase = preceding_phase(first_phase);
            let start_price = bars
                .first()
                .map(bar_low)
                .filter(|p| p.is_finite() && *p > 0.0)
                .unwrap_or(first_seg.start_price);
            let leading = open_segment(
                leading_phase,
                WyckoffCycleSource::Elliott,
                0,
                start_price,
                None,
            );
            out.push(close_segment(
                leading,
                first_seg.start_bar,
                first_seg.start_price,
                bars,
            ));
        }
    }

    for seg in sorted {
        // Fill gap with a transition tile (Distribution or Accumulation)
        // when the previous tile ended before this segment starts.
        if let Some((p_end, p_price, p_phase)) = prev_end {
            if seg.start_bar > p_end + gap_min_bars {
                let transition = transition_phase(p_phase);
                out.push(close_segment(
                    open_segment(
                        transition,
                        WyckoffCycleSource::Elliott,
                        p_end,
                        p_price,
                        None,
                    ),
                    seg.start_bar,
                    seg.start_price,
                    bars,
                ));
            }
        }

        let phase = phase_for_segment(seg);
        let tile = open_segment(
            phase,
            WyckoffCycleSource::Elliott,
            seg.start_bar,
            seg.start_price,
            seg.source_id.clone(),
        );
        let closed =
            close_segment(tile, seg.end_bar, seg.end_price, bars);
        prev_end = Some((closed.end_bar, closed.end_price, closed.phase));
        out.push(closed);
    }

    // Trailing fill — extend the last tile (or its transition) to the
    // tape head so the chart has continuous coverage.
    if let Some((p_end, p_price, p_phase)) = prev_end {
        if tape_end_bar > p_end + gap_min_bars {
            let transition = transition_phase(p_phase);
            let mut trailing = open_segment(
                transition,
                WyckoffCycleSource::Elliott,
                p_end,
                p_price,
                None,
            );
            trailing.end_bar = tape_end_bar;
            trailing.end_price = tape_end_price;
            let (hi, lo) = compute_bounds(
                bars,
                trailing.start_bar,
                trailing.end_bar,
                trailing.start_price,
            );
            trailing.phase_high = hi;
            trailing.phase_low = lo;
            // Trailing fill is "open" — completed flag stays false.
            out.push(trailing);
        }
    }

    out
}

fn phase_for_segment(seg: &ElliottSegment) -> WyckoffCyclePhase {
    match (seg.kind, seg.bullish) {
        (ElliottSegmentKind::Motive, true) => WyckoffCyclePhase::Markup,
        (ElliottSegmentKind::Motive, false) => WyckoffCyclePhase::Markdown,
        (ElliottSegmentKind::Abc, true) => WyckoffCyclePhase::Accumulation,
        (ElliottSegmentKind::Abc, false) => WyckoffCyclePhase::Markdown,
    }
}

/// Phase that fills the gap AFTER a tile of `prev` phase. Markup ends
/// at a top → Distribution; Markdown ends at a bottom → Accumulation.
fn transition_phase(prev: WyckoffCyclePhase) -> WyckoffCyclePhase {
    match prev {
        WyckoffCyclePhase::Markup | WyckoffCyclePhase::Distribution => {
            WyckoffCyclePhase::Distribution
        }
        WyckoffCyclePhase::Markdown | WyckoffCyclePhase::Accumulation => {
            WyckoffCyclePhase::Accumulation
        }
    }
}

/// Phase that PRECEDES the given phase in the canonical 4-phase
/// rotation (Pruden). Used by `detect_cycles_from_elliott` to fill
/// the LEADING gap before the first Elliott segment — every bull
/// motive launches from an Accumulation base, every bear leg from a
/// Distribution top.
///
///   Markup       ← Accumulation  (impulse launchpad — the most
///                                 actionable signal: detecting
///                                 Accumulation = anticipating the
///                                 next Markup before W3 ignites)
///   Distribution ← Markup        (rally tops out → distribution)
///   Markdown     ← Distribution  (B-wave fakeout → C decline)
///   Accumulation ← Markdown      (C bottom → basing)
fn preceding_phase(next: WyckoffCyclePhase) -> WyckoffCyclePhase {
    match next {
        WyckoffCyclePhase::Markup => WyckoffCyclePhase::Accumulation,
        WyckoffCyclePhase::Distribution => WyckoffCyclePhase::Markup,
        WyckoffCyclePhase::Markdown => WyckoffCyclePhase::Distribution,
        WyckoffCyclePhase::Accumulation => WyckoffCyclePhase::Markdown,
    }
}

// ── Confluence merge ──────────────────────────────────────────────

/// Combine event-driven and Elliott-anchored tiles. When both sources
/// produce a tile of the SAME phase whose time-windows overlap by at
/// least `min_overlap_ratio` of the shorter tile, emit a single
/// `Confluent` tile spanning the UNION of both windows. Unmatched
/// tiles retain their source tag and are also returned.
///
/// `min_overlap_ratio` ∈ [0.0, 1.0]. 0.5 = "at least half of the
/// shorter tile must overlap with the longer".
pub fn merge_cycles_with_confluence(
    event_cycles: Vec<WyckoffCycle>,
    elliott_cycles: Vec<WyckoffCycle>,
    bars: &[Bar],
    min_overlap_ratio: f64,
) -> Vec<WyckoffCycle> {
    let mut out: Vec<WyckoffCycle> = Vec::new();
    let mut elliott_used: Vec<bool> = vec![false; elliott_cycles.len()];

    for ev in event_cycles.into_iter() {
        let mut matched_idx: Option<usize> = None;
        for (j, el) in elliott_cycles.iter().enumerate() {
            if elliott_used[j] {
                continue;
            }
            if el.phase != ev.phase {
                continue;
            }
            let overlap = overlap_ratio(
                ev.start_bar,
                ev.end_bar,
                el.start_bar,
                el.end_bar,
            );
            if overlap >= min_overlap_ratio {
                matched_idx = Some(j);
                break;
            }
        }
        match matched_idx {
            Some(j) => {
                elliott_used[j] = true;
                let el = &elliott_cycles[j];
                let start_bar = ev.start_bar.min(el.start_bar);
                let end_bar = ev.end_bar.max(el.end_bar);
                let start_price =
                    if ev.start_bar <= el.start_bar { ev.start_price } else { el.start_price };
                let end_price =
                    if ev.end_bar >= el.end_bar { ev.end_price } else { el.end_price };
                let (hi, lo) =
                    compute_bounds(bars, start_bar, end_bar, start_price);
                out.push(WyckoffCycle {
                    phase: ev.phase,
                    source: WyckoffCycleSource::Confluent,
                    start_bar,
                    end_bar,
                    start_price,
                    end_price,
                    phase_high: hi,
                    phase_low: lo,
                    completed: ev.completed && el.completed,
                    source_pattern_id: el.source_pattern_id.clone(),
                });
            }
            None => out.push(ev),
        }
    }

    // Unmatched Elliott tiles flow through with their own source tag.
    for (j, el) in elliott_cycles.into_iter().enumerate() {
        if !elliott_used[j] {
            out.push(el);
        }
    }
    out.sort_by_key(|c| c.start_bar);
    out
}

fn overlap_ratio(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> f64 {
    let lo = a_start.max(b_start);
    let hi = a_end.min(b_end);
    if hi <= lo {
        return 0.0;
    }
    let intersection = (hi - lo) as f64;
    let a_dur = (a_end.saturating_sub(a_start)).max(1) as f64;
    let b_dur = (b_end.saturating_sub(b_start)).max(1) as f64;
    let shorter = a_dur.min(b_dur);
    intersection / shorter
}

// ── Helpers ───────────────────────────────────────────────────────

fn open_segment(
    phase: WyckoffCyclePhase,
    source: WyckoffCycleSource,
    bar: usize,
    price: f64,
    source_pattern_id: Option<String>,
) -> WyckoffCycle {
    WyckoffCycle {
        phase,
        source,
        start_bar: bar,
        end_bar: bar,
        start_price: price,
        end_price: price,
        phase_high: price,
        phase_low: price,
        completed: false,
        source_pattern_id,
    }
}

fn close_segment(
    mut cur: WyckoffCycle,
    end_bar: usize,
    end_price: f64,
    bars: &[Bar],
) -> WyckoffCycle {
    cur.end_bar = end_bar.max(cur.start_bar);
    cur.end_price = end_price;
    cur.completed = true;
    let (hi, lo) =
        compute_bounds(bars, cur.start_bar, cur.end_bar, cur.start_price);
    cur.phase_high = hi;
    cur.phase_low = lo;
    cur
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

    fn motive(bullish: bool, start: usize, end: usize, sp: f64, ep: f64) -> ElliottSegment {
        ElliottSegment {
            kind: ElliottSegmentKind::Motive,
            bullish,
            start_bar: start,
            end_bar: end,
            start_price: sp,
            end_price: ep,
            source_id: Some(format!("m-{start}")),
        }
    }
    fn abc(bullish: bool, start: usize, end: usize, sp: f64, ep: f64) -> ElliottSegment {
        ElliottSegment {
            kind: ElliottSegmentKind::Abc,
            bullish,
            start_bar: start,
            end_bar: end,
            start_price: sp,
            end_price: ep,
            source_id: Some(format!("a-{start}")),
        }
    }

    #[test]
    fn event_driven_full_rotation() {
        let events = vec![
            ev(WyckoffEventKind::Sc, 10, 100.0),
            ev(WyckoffEventKind::Bu, 30, 110.0),
            ev(WyckoffEventKind::Bc, 60, 140.0),
            ev(WyckoffEventKind::Sow, 80, 130.0),
            ev(WyckoffEventKind::Sc, 110, 95.0),
        ];
        let cycles = detect_cycles(&events, &[], 200, 100.0);
        assert_eq!(cycles.len(), 5);
        assert!(cycles.iter().all(|c| c.source == WyckoffCycleSource::Event));
    }

    #[test]
    fn leading_fill_emits_accumulation_before_first_motive() {
        // Bull motive starts at bar 50; leading fill should emit an
        // Accumulation tile [0, 50] BEFORE the Markup tile.
        let segs = vec![motive(true, 50, 90, 100.0, 150.0)];
        let cycles = detect_cycles_from_elliott(&segs, &[], 200, 140.0);
        assert!(cycles.len() >= 2);
        assert_eq!(cycles[0].phase, WyckoffCyclePhase::Accumulation);
        assert_eq!(cycles[0].start_bar, 0);
        assert_eq!(cycles[0].end_bar, 50);
        assert_eq!(cycles[1].phase, WyckoffCyclePhase::Markup);
    }

    #[test]
    fn leading_fill_emits_distribution_before_first_bearish_abc() {
        let segs = vec![abc(false, 50, 90, 150.0, 120.0)];
        let cycles = detect_cycles_from_elliott(&segs, &[], 200, 110.0);
        assert!(cycles.len() >= 2);
        assert_eq!(cycles[0].phase, WyckoffCyclePhase::Distribution);
        assert_eq!(cycles[0].start_bar, 0);
        assert_eq!(cycles[1].phase, WyckoffCyclePhase::Markdown);
    }

    #[test]
    fn elliott_anchored_motive_then_abc() {
        let segs = vec![
            motive(true, 10, 50, 100.0, 150.0),
            abc(false, 50, 80, 150.0, 120.0),
        ];
        let cycles = detect_cycles_from_elliott(&segs, &[], 200, 130.0);
        // Leading Accumulation (bar 0→10), Markup, Markdown,
        // trailing Accumulation (after C bottom).
        assert!(cycles.len() >= 3);
        assert_eq!(cycles[0].phase, WyckoffCyclePhase::Accumulation);
        assert_eq!(cycles[1].phase, WyckoffCyclePhase::Markup);
        assert_eq!(cycles[2].phase, WyckoffCyclePhase::Markdown);
        assert!(cycles.iter().all(|c| c.source == WyckoffCycleSource::Elliott));
    }

    #[test]
    fn elliott_fills_gap_with_transition() {
        // Motive ends at 50, next motive at 100 → 50-bar gap fills
        // with Distribution (Markup → Distribution transition).
        let segs = vec![
            motive(true, 10, 50, 100.0, 150.0),
            motive(true, 100, 140, 145.0, 200.0),
        ];
        let cycles = detect_cycles_from_elliott(&segs, &[], 200, 200.0);
        let phases: Vec<_> = cycles.iter().map(|c| c.phase).collect();
        assert!(phases.contains(&WyckoffCyclePhase::Distribution));
    }

    #[test]
    fn confluence_merges_overlapping_same_phase() {
        let event_cycles = vec![WyckoffCycle {
            phase: WyckoffCyclePhase::Markup,
            source: WyckoffCycleSource::Event,
            start_bar: 20,
            end_bar: 60,
            start_price: 100.0,
            end_price: 140.0,
            phase_high: 145.0,
            phase_low: 98.0,
            completed: true,
            source_pattern_id: None,
        }];
        let elliott_cycles = vec![WyckoffCycle {
            phase: WyckoffCyclePhase::Markup,
            source: WyckoffCycleSource::Elliott,
            start_bar: 15,
            end_bar: 65,
            start_price: 95.0,
            end_price: 145.0,
            phase_high: 148.0,
            phase_low: 92.0,
            completed: true,
            source_pattern_id: Some("m-15".into()),
        }];
        let merged =
            merge_cycles_with_confluence(event_cycles, elliott_cycles, &[], 0.5);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].source, WyckoffCycleSource::Confluent);
        assert_eq!(merged[0].start_bar, 15); // union start
        assert_eq!(merged[0].end_bar, 65); // union end
        assert_eq!(merged[0].source_pattern_id.as_deref(), Some("m-15"));
    }

    #[test]
    fn confluence_keeps_unmatched_tiles() {
        let event_cycles = vec![WyckoffCycle {
            phase: WyckoffCyclePhase::Markup,
            source: WyckoffCycleSource::Event,
            start_bar: 10,
            end_bar: 30,
            start_price: 100.0,
            end_price: 110.0,
            phase_high: 112.0,
            phase_low: 99.0,
            completed: true,
            source_pattern_id: None,
        }];
        let elliott_cycles = vec![WyckoffCycle {
            phase: WyckoffCyclePhase::Markdown,
            source: WyckoffCycleSource::Elliott,
            start_bar: 100,
            end_bar: 150,
            start_price: 200.0,
            end_price: 150.0,
            phase_high: 200.0,
            phase_low: 145.0,
            completed: true,
            source_pattern_id: Some("a-100".into()),
        }];
        let merged =
            merge_cycles_with_confluence(event_cycles, elliott_cycles, &[], 0.5);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].source, WyckoffCycleSource::Event);
        assert_eq!(merged[1].source, WyckoffCycleSource::Elliott);
    }

    #[test]
    fn slot_filter_drops_weak_events() {
        let mut events = vec![
            ev(WyckoffEventKind::Sc, 10, 100.0),
            ev(WyckoffEventKind::Bc, 60, 140.0),
        ];
        events[0].score = 0.6;
        events[1].score = 0.9;
        let z0 = detect_cycles_for_slot(&events, &[], 0.55, 200, 100.0);
        let z3 = detect_cycles_for_slot(&events, &[], 0.85, 200, 100.0);
        assert_eq!(z0.len(), 2);
        assert_eq!(z3.len(), 1);
        assert_eq!(z3[0].phase, WyckoffCyclePhase::Distribution);
    }
}
