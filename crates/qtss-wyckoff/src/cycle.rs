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
/// ('motive','abc') AND slot = N` and parsing the JSONB `anchors`
/// array.
///
/// `wave_anchors` carries each sub-wave anchor as `(bar_index, price)`
/// so the cycle detector can emit phase tiles at the correct
/// SUB-WAVE positions:
///   * Motive (6 anchors): W0, W1, W2, W3, W4, W5
///   * ABC    (4 anchors): X0, A,  B,  C
/// When `wave_anchors` is empty the detector falls back to the
/// whole-segment mapping (single tile per segment).
#[derive(Debug, Clone, PartialEq)]
pub struct ElliottSegment {
    pub kind: ElliottSegmentKind,
    pub bullish: bool,
    pub start_bar: usize,
    pub end_bar: usize,
    pub start_price: f64,
    pub end_price: f64,
    pub source_id: Option<String>,
    pub wave_anchors: Vec<(usize, f64)>,
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
                // BUG4 — symmetric Markdown inference. SC marks the
                // bottom of a downtrend leg; if the prev phase was
                // Distribution and SoW didn't fire, insert an
                // inferred Markdown [distribution_end, SC] so the
                // rotation alternates Acc → Markup → Dist →
                // Markdown → Acc as Wyckoff demands.
                if let Some(prev) = current.take() {
                    let prev_phase = prev.phase;
                    let prev_start = prev.start_bar;
                    out.push(close_segment(
                        prev,
                        ev.bar_index,
                        ev.reference_price,
                        bars,
                    ));
                    if prev_phase == WyckoffCyclePhase::Distribution {
                        // Mirror of the BC inference: scan for the
                        // HIGHEST high in the prior Distribution
                        // window — that's where the markdown leg
                        // launched from.
                        let scan_lo = prev_start.max(0);
                        let scan_hi = ev.bar_index;
                        let mut max_idx = scan_hi.saturating_sub(1);
                        let mut max_price = f64::NEG_INFINITY;
                        for bar_idx in scan_lo..scan_hi {
                            if let Some(b) = bars.get(bar_idx) {
                                let h = bar_high(b);
                                if h > max_price {
                                    max_price = h;
                                    max_idx = bar_idx;
                                }
                            }
                        }
                        if !max_price.is_finite() {
                            max_price = ev.reference_price;
                        }
                        if max_idx <= prev_start {
                            max_idx = prev_start + 1;
                        }
                        if max_idx < ev.bar_index {
                            let inferred = open_segment(
                                WyckoffCyclePhase::Markdown,
                                WyckoffCycleSource::Event,
                                max_idx,
                                max_price,
                                None,
                            );
                            out.push(close_segment(
                                inferred,
                                ev.bar_index,
                                ev.reference_price,
                                bars,
                            ));
                        }
                    }
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
                // BUG4 — Wyckoff doctrine demands a Markup leg
                // BETWEEN Accumulation and the BC top. When the
                // event tape skips Bu / SoS (common — Bu fires
                // less reliably than the climax events), the
                // state machine used to leap straight from
                // Accumulation to Distribution and the entire
                // bullish leg got tagged Distribution by mistake.
                // Insert an INFERRED Markup tile from the close of
                // Accumulation to the BC bar so the rotation
                // alternates correctly.
                if let Some(prev) = current.take() {
                    let prev_phase = prev.phase;
                    let prev_start = prev.start_bar;
                    out.push(close_segment(
                        prev,
                        ev.bar_index,
                        ev.reference_price,
                        bars,
                    ));
                    if prev_phase == WyckoffCyclePhase::Accumulation {
                        // Inferred Markup span = from the LOWEST low
                        // in the prior Accumulation window to the BC
                        // bar. That low is the structural launchpad
                        // of the impulse (Spring / Test / first
                        // breakout HL). Wider than 1 bar so the
                        // rendered tile covers the actual rally.
                        let scan_lo = prev_start.max(0);
                        let scan_hi = ev.bar_index;
                        let mut min_idx = scan_hi.saturating_sub(1);
                        let mut min_price = f64::INFINITY;
                        for bar_idx in scan_lo..scan_hi {
                            if let Some(b) = bars.get(bar_idx) {
                                let l = bar_low(b);
                                if l < min_price {
                                    min_price = l;
                                    min_idx = bar_idx;
                                }
                            }
                        }
                        if !min_price.is_finite() {
                            min_price = ev.reference_price;
                        }
                        // Cap the start so we never overlap the
                        // earlier accumulation tile fully — keep the
                        // inferred Markup AFTER prev.start_bar.
                        if min_idx <= prev_start {
                            min_idx = prev_start + 1;
                        }
                        if min_idx < ev.bar_index {
                            let inferred = open_segment(
                                WyckoffCyclePhase::Markup,
                                WyckoffCycleSource::Event,
                                min_idx,
                                min_price,
                                None,
                            );
                            out.push(close_segment(
                                inferred,
                                ev.bar_index,
                                ev.reference_price,
                                bars,
                            ));
                        }
                    }
                }
                current = Some(open_segment(
                    WyckoffCyclePhase::Distribution,
                    WyckoffCycleSource::Event,
                    ev.bar_index,
                    ev.reference_price,
                    None,
                ));
            }
            // BUG4 — Wyckoff Phase D Markup-ignition signal set:
            //   Bu  (Jump-Across-Creek / Backup) — range top broken
            //   Sos (Sign of Strength) — first wide bull bar
            //   Lps (Last Point of Support) — higher-low after Bu
            // All three are equivalent triggers for the Acc → Markup
            // transition. The user asked "marketup başlangıcını
            // anlayacağımız sinyal yok mu" — these three (plus the
            // existing Spring / Test Phase C signals that prep the
            // launchpad) are the canonical answer; the cycle state
            // machine accepts all of them as Markup openers.
            WyckoffEventKind::Bu
            | WyckoffEventKind::Sos
            | WyckoffEventKind::Lps => {
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
            // BUG4 — Sow (Sign of Weakness) opens Markdown when prev
            // is Distribution. LPSY (bear-variant Lps) is also a
            // valid Markdown trigger but it shares the
            // WyckoffEventKind::Lps tag with the bull-side LPS used
            // by the Markup branch above; keeping Lps in the Markup
            // branch only is fine because the prev_phase check on
            // that branch fails when prev = Distribution, so the
            // bear LPSY event simply doesn't transition (Sow does
            // the work). Adding Lps here would create a duplicate
            // match arm — Rust rejects it.
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
            // Bound leading window — small fraction of segment
            // duration so the basing/topping zone stays a tight
            // contextual box, not a slab. Previous formula
            // (lookback = seg_dur capped at 500) painted 4-6 month
            // boxes on 1d which user flagged as "bu kadar büyük
            // accumulation olmaz". 1/3 of segment duration with a
            // 60-bar absolute cap gives ~3-week basing zones on 4h
            // and ~2-month basing zones on 1d.
            let seg_dur = first_seg.end_bar.saturating_sub(first_seg.start_bar);
            let lookback = (seg_dur / 3).max(20).min(60);
            let leading_start = first_seg.start_bar.saturating_sub(lookback);
            let start_price = bars
                .get(leading_start)
                .map(bar_low)
                .filter(|p| p.is_finite() && *p > 0.0)
                .or_else(|| bars.first().map(bar_low))
                .filter(|p| p.is_finite() && *p > 0.0)
                .unwrap_or(first_seg.start_price);
            let leading = open_segment(
                leading_phase,
                WyckoffCycleSource::Elliott,
                leading_start,
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

    for seg in &sorted {
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

        // Pruden's sub-wave mapping (when all anchors are available):
        //   Bullish motive  W0..W5  →  Accumulation [W0,W2] + Markup     [W2,W5]
        //   Bearish ABC     X0..C   →  Distribution [X0,B]  + Markdown   [B,C]
        //   Bearish motive  W0..W5  →  Distribution [W0,W2] + Markdown   [W2,W5]
        //   Bullish ABC     X0..C   →  Markdown    [X0,B]   + Accumulation [B,C]
        // Falls back to whole-segment mapping when sub-wave anchors
        // are missing (legacy rows / partial detections).
        let tiles_emitted = emit_subwave_tiles(seg, bars, &mut out);
        if !tiles_emitted {
            // Fallback: single tile spanning the entire segment.
            let phase = phase_for_segment(seg);
            let tile = open_segment(
                phase,
                WyckoffCycleSource::Elliott,
                seg.start_bar,
                seg.start_price,
                seg.source_id.clone(),
            );
            out.push(close_segment(
                tile,
                seg.end_bar,
                seg.end_price,
                bars,
            ));
        }
        // The "previous end" tracker uses the FINAL tile we emitted —
        // that's what gap-fill / trailing-fill chain off.
        if let Some(last) = out.last() {
            prev_end = Some((last.end_bar, last.end_price, last.phase));
        }
    }

    // Trailing fill — extend the last tile to the tape head, but
    // bound the extension so a slot that hasn't seen a new Elliott
    // segment in months doesn't paint a 12+ month "Distribution"
    // tile (user flagged this on 1d Z5: cycle_distribution
    // 2025-01-20 → 2026-04-26 = 15 months). Cap to 3× the last
    // segment's duration with a hard min/max [30, 250] bars.
    if let Some((p_end, p_price, p_phase)) = prev_end {
        if tape_end_bar > p_end + gap_min_bars {
            let transition = transition_phase(p_phase);
            let last_dur = sorted
                .last()
                .map(|s| s.end_bar.saturating_sub(s.start_bar))
                .unwrap_or(60);
            let max_trail = (last_dur * 3).max(30).min(250);
            let trailing_end = (p_end + max_trail).min(tape_end_bar);
            let trailing_end_price = if trailing_end >= tape_end_bar {
                tape_end_price
            } else {
                bars.get(trailing_end)
                    .map(bar_low)
                    .filter(|p| p.is_finite() && *p > 0.0)
                    .unwrap_or(p_price)
            };
            let mut trailing = open_segment(
                transition,
                WyckoffCycleSource::Elliott,
                p_end,
                p_price,
                None,
            );
            trailing.end_bar = trailing_end;
            trailing.end_price = trailing_end_price;
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

/// Emit per-segment tiles using sub-wave anchors (Pruden mapping).
/// Returns `true` when sub-wave tiles were emitted, `false` if the
/// segment lacked enough anchors and the caller should fall back to
/// the whole-segment mapping.
///
/// Mapping (anchor indices):
///   Bullish motive  (W0..W5):  Accumulation [W0,W2] + Markup     [W2,W5]
///   Bearish motive  (W0..W5):  Distribution [W0,W2] + Markdown   [W2,W5]
///   Bearish ABC     (X0..C):   Distribution [X0,B]  + Markdown   [B,C]
///   Bullish ABC     (X0..C):   Markdown     [X0,B]  + Accumulation [B,C]
fn emit_subwave_tiles(
    seg: &ElliottSegment,
    bars: &[Bar],
    out: &mut Vec<WyckoffCycle>,
) -> bool {
    let phases = match (seg.kind, seg.bullish, seg.wave_anchors.len()) {
        // Motive needs 6 anchors (W0..W5). Tile A = [W0,W2], B = [W2,W5].
        (ElliottSegmentKind::Motive, true, 6) => Some((
            (0usize, 2usize, WyckoffCyclePhase::Accumulation),
            (2usize, 5usize, WyckoffCyclePhase::Markup),
        )),
        (ElliottSegmentKind::Motive, false, 6) => Some((
            (0, 2, WyckoffCyclePhase::Distribution),
            (2, 5, WyckoffCyclePhase::Markdown),
        )),
        // ABC needs 4 anchors (X0, A, B, C). Tile A = [X0,B], B = [B,C].
        (ElliottSegmentKind::Abc, false, 4) => Some((
            (0, 2, WyckoffCyclePhase::Distribution),
            (2, 3, WyckoffCyclePhase::Markdown),
        )),
        (ElliottSegmentKind::Abc, true, 4) => Some((
            (0, 2, WyckoffCyclePhase::Markdown),
            (2, 3, WyckoffCyclePhase::Accumulation),
        )),
        _ => None,
    };
    let Some(((a0, a1, p_a), (b0, b1, p_b))) = phases else {
        return false;
    };
    let wa = &seg.wave_anchors;
    let (a_start_bar, a_start_price) = wa[a0];
    let (a_end_bar, a_end_price) = wa[a1];
    let (b_start_bar, b_start_price) = wa[b0];
    let (b_end_bar, b_end_price) = wa[b1];

    let tile_a = open_segment(
        p_a,
        WyckoffCycleSource::Elliott,
        a_start_bar,
        a_start_price,
        seg.source_id.clone(),
    );
    out.push(close_segment(tile_a, a_end_bar, a_end_price, bars));

    let tile_b = open_segment(
        p_b,
        WyckoffCycleSource::Elliott,
        b_start_bar,
        b_start_price,
        seg.source_id.clone(),
    );
    out.push(close_segment(tile_b, b_end_bar, b_end_price, bars));

    true
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

/// Boost an Elliott-anchored Markup/Markdown tile to `Confluent` when
/// its START aligns (within `boost_window_bars`) with a Spring (for
/// Markup) or UTAD (for Markdown) event. Pruden's canonical mapping:
/// Spring is the Phase-C bottom that ignites Markup; UTAD is the
/// Phase-C top that triggers Markdown. Co-occurrence of these
/// volume-validated events with the structural sub-wave anchor is the
/// highest-conviction trade signal in Wyckoff doctrine.
pub fn boost_with_phase_c_events(
    cycles: Vec<WyckoffCycle>,
    events: &[WyckoffEvent],
    boost_window_bars: usize,
) -> Vec<WyckoffCycle> {
    cycles
        .into_iter()
        .map(|c| {
            // Only Elliott-anchored Markup / Markdown tiles are
            // candidates — Event-anchored tiles already have their
            // own confluence semantics, and ranges (Accum / Dist)
            // aren't ignited by Spring / UTAD.
            if c.source != WyckoffCycleSource::Elliott {
                return c;
            }
            let target_kind = match c.phase {
                WyckoffCyclePhase::Markup => WyckoffEventKind::Spring,
                WyckoffCyclePhase::Markdown => WyckoffEventKind::Utad,
                _ => return c,
            };
            let aligned = events.iter().any(|e| {
                e.kind == target_kind
                    && e.bar_index <= c.start_bar + boost_window_bars
                    && c.start_bar <= e.bar_index + boost_window_bars
            });
            if aligned {
                WyckoffCycle {
                    source: WyckoffCycleSource::Confluent,
                    ..c
                }
            } else {
                c
            }
        })
        .collect()
}

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
                // FAZ 25.4.E — Elliott bounds are AUTHORITATIVE for
                // the confluent tile; event presence only upgrades the
                // source tag. Earlier behaviour took union (min start
                // / max end) which let the BC event (often firing a
                // few bars BEFORE W5) bleed Distribution into the
                // Markup zone — the user observed a Distribution box
                // starting 3 days before W5 on BTC 4h Z4.
                //
                // Structural sub-wave anchors (W2/W5 for motive,
                // X0/B/C for ABC) define the phase boundary; the
                // event timing is a *confirmation* of that boundary,
                // not a redefinition of it.
                out.push(WyckoffCycle {
                    phase: el.phase,
                    source: WyckoffCycleSource::Confluent,
                    start_bar: el.start_bar,
                    end_bar: el.end_bar,
                    start_price: el.start_price,
                    end_price: el.end_price,
                    phase_high: el.phase_high,
                    phase_low: el.phase_low,
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

/// BUG3 — Coalesce consecutive same-phase tiles into a single
/// spanning tile.
///
/// `merge_cycles_with_confluence` only pairs ONE event tile with ONE
/// Elliott tile by overlap. When the Elliott detector emits multiple
/// adjacent same-phase tiles (e.g. several ABC corrections within an
/// Accumulation base, or two consecutive motives both labelled
/// Markup), they all flow through unchanged and the chart renders 2-3
/// boxes stacked on top of each other for the same macro phase.
///
/// Wyckoff doctrine: the four-phase cycle is a strict alternation
/// (Acc → Markup → Dist → Markdown → Acc → …). Two consecutive
/// Accumulation tiles without an intervening Markup is structurally
/// invalid; they belong to the SAME accumulation range.
///
/// This pass:
///   1. Sorts by start_bar.
///   2. For each pair of adjacent tiles with identical phase, fuses
///      them: span = [min start, max end], price band = [min low,
///      max high]. Source upgrades by priority Confluent > Elliott
///      > Event so the highest-conviction tag survives.
///   3. Distinct phases stay separated.
///
/// Pure function — preserves the input ordering invariant the
/// downstream writer expects (sorted by start_bar).
pub fn dedupe_consecutive_same_phase(
    cycles: Vec<WyckoffCycle>,
) -> Vec<WyckoffCycle> {
    let mut sorted = cycles;
    sorted.sort_by_key(|c| (c.start_bar, c.end_bar));
    let mut out: Vec<WyckoffCycle> = Vec::new();
    for c in sorted {
        match out.last_mut() {
            Some(prev) if prev.phase == c.phase => {
                // Same phase consecutive → fuse.
                if c.end_bar > prev.end_bar {
                    prev.end_bar = c.end_bar;
                    prev.end_price = c.end_price;
                }
                if c.phase_high > prev.phase_high {
                    prev.phase_high = c.phase_high;
                }
                if c.phase_low < prev.phase_low {
                    prev.phase_low = c.phase_low;
                }
                prev.source = source_priority(prev.source, c.source);
                // The fused tile is "completed" only when the LAST
                // contributing tile says so (chronologically).
                prev.completed = c.completed;
                if prev.source_pattern_id.is_none() {
                    prev.source_pattern_id = c.source_pattern_id;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn source_priority(
    a: WyckoffCycleSource,
    b: WyckoffCycleSource,
) -> WyckoffCycleSource {
    if source_rank(a) >= source_rank(b) {
        a
    } else {
        b
    }
}

fn source_rank(s: WyckoffCycleSource) -> u8 {
    match s {
        WyckoffCycleSource::Confluent => 3,
        WyckoffCycleSource::Elliott => 2,
        WyckoffCycleSource::Event => 1,
    }
}

/// BUG3 (round 2 — 2026-04-27) — Resolve overlapping DIFFERENT-phase
/// tiles by source priority + length tiebreak.
///
/// `merge_cycles_with_confluence` only pairs SAME-phase tiles. When
/// the event-driven detector flags Distribution [bar 100, 200] and
/// the Elliott-anchored detector flags Markdown [bar 110, 250] for
/// the same slot, the merger's phase guard rejects both, and they
/// flow through as separate overlapping boxes. The chart then shows
/// nested rectangles for the SAME time period — exactly what the
/// user reported with ETHUSDT 1h Z1: a giant Distribution box with
/// Markdown and Accumulation tiles drawn INSIDE it.
///
/// Wyckoff doctrine demands a strict phase alternation per slot:
/// Accumulation → Markup → Distribution → Markdown → Accumulation
/// (or substages of those). At any single bar there is exactly ONE
/// active phase; concurrent phases are an artefact of two
/// independent detection paths emitting their own opinion.
///
/// This pass enforces a single non-overlapping timeline by:
///   1. Sorting tiles by source priority DESC (Confluent > Elliott >
///      Event), then by length DESC (longer = more confident).
///   2. Walking the sorted list — each tile is "placed" by trimming
///      it against everything already placed. Higher-priority tiles
///      always survive intact; lower-priority tiles get clipped to
///      the gaps between higher-priority neighbours.
///   3. A tile that gets fully covered (no gap left) is dropped.
///
/// The output is sorted by start_bar and never has two tiles whose
/// [start, end] ranges overlap. Different-phase trimming preserves
/// the macroscopic Wyckoff narrative: where event and Elliott
/// disagree on phase boundaries, the higher-conviction (Confluent)
/// boundary wins, and the lower-conviction tile compresses to the
/// margins where the higher-priority detector had no opinion.
pub fn enforce_non_overlap(
    cycles: Vec<WyckoffCycle>,
) -> Vec<WyckoffCycle> {
    if cycles.is_empty() {
        return cycles;
    }
    let mut sorted = cycles;
    // Priority DESC, then length DESC (longer tiles more likely to
    // be the macroscopic phase rather than a sub-event).
    sorted.sort_by(|a, b| {
        let ra = source_rank(a.source);
        let rb = source_rank(b.source);
        rb.cmp(&ra).then_with(|| {
            let la = a.end_bar.saturating_sub(a.start_bar);
            let lb = b.end_bar.saturating_sub(b.start_bar);
            lb.cmp(&la)
        })
    });

    let mut placed: Vec<WyckoffCycle> = Vec::new();

    for c in sorted {
        // Trim c against every already-placed tile. We keep a list
        // of "alive" sub-segments of c that have not yet been
        // claimed by a higher-priority neighbour.
        let mut alive: Vec<(usize, usize)> =
            vec![(c.start_bar, c.end_bar)];
        for p in &placed {
            let mut next_alive: Vec<(usize, usize)> =
                Vec::with_capacity(alive.len() + 1);
            for (s, e) in &alive {
                let s = *s;
                let e = *e;
                let ovs = s.max(p.start_bar);
                let ove = e.min(p.end_bar);
                if ovs >= ove {
                    // No intersection — keep segment whole.
                    next_alive.push((s, e));
                    continue;
                }
                // Intersection [ovs, ove] is owned by p — drop it
                // from c. Keep the parts before and after.
                if s < ovs {
                    next_alive.push((s, ovs));
                }
                if ove < e {
                    next_alive.push((ove, e));
                }
            }
            alive = next_alive;
            if alive.is_empty() {
                break;
            }
        }

        // Emit one clipped tile per surviving segment.
        for (s, e) in alive {
            if e > s {
                let mut clone = c.clone();
                clone.start_bar = s;
                clone.end_bar = e;
                placed.push(clone);
            }
        }
    }

    placed.sort_by_key(|c| c.start_bar);
    placed
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
            wave_anchors: Vec::new(),
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
            wave_anchors: Vec::new(),
        }
    }
    fn motive_with_waves(
        bullish: bool,
        start: usize,
        end: usize,
        sp: f64,
        ep: f64,
        anchors: Vec<(usize, f64)>,
    ) -> ElliottSegment {
        ElliottSegment {
            kind: ElliottSegmentKind::Motive,
            bullish,
            start_bar: start,
            end_bar: end,
            start_price: sp,
            end_price: ep,
            source_id: Some(format!("m-{start}")),
            wave_anchors: anchors,
        }
    }
    fn abc_with_waves(
        bullish: bool,
        start: usize,
        end: usize,
        sp: f64,
        ep: f64,
        anchors: Vec<(usize, f64)>,
    ) -> ElliottSegment {
        ElliottSegment {
            kind: ElliottSegmentKind::Abc,
            bullish,
            start_bar: start,
            end_bar: end,
            start_price: sp,
            end_price: ep,
            source_id: Some(format!("a-{start}")),
            wave_anchors: anchors,
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
        // Bull motive [800, 840] (40-bar dur) — leading lookback =
        // max(40/3, 20).min(60) = 20 bars, so Accumulation [780, 800].
        let segs = vec![motive(true, 800, 840, 100.0, 150.0)];
        let cycles = detect_cycles_from_elliott(&segs, &[], 1000, 140.0);
        assert!(cycles.len() >= 2);
        assert_eq!(cycles[0].phase, WyckoffCyclePhase::Accumulation);
        assert_eq!(cycles[0].start_bar, 780);
        assert_eq!(cycles[0].end_bar, 800);
        assert_eq!(cycles[1].phase, WyckoffCyclePhase::Markup);
    }

    #[test]
    fn leading_fill_emits_distribution_before_first_bearish_abc() {
        let segs = vec![abc(false, 800, 840, 150.0, 120.0)];
        let cycles = detect_cycles_from_elliott(&segs, &[], 1000, 110.0);
        assert!(cycles.len() >= 2);
        assert_eq!(cycles[0].phase, WyckoffCyclePhase::Distribution);
        assert_eq!(cycles[0].start_bar, 780); // 800 - 20
        assert_eq!(cycles[1].phase, WyckoffCyclePhase::Markdown);
    }

    #[test]
    fn subwave_motive_emits_accumulation_then_markup() {
        // Bull motive with full W0..W5 anchors → emits Accumulation
        // [W0,W2] then Markup [W2,W5]. No fallback Markup-of-whole.
        let waves = vec![
            (50, 100.0),  // W0
            (60, 110.0),  // W1
            (65, 102.0),  // W2 (Spring zone)
            (80, 145.0),  // W3 (ignition top)
            (85, 130.0),  // W4
            (90, 150.0),  // W5
        ];
        let segs = vec![motive_with_waves(true, 50, 90, 100.0, 150.0, waves)];
        let cycles = detect_cycles_from_elliott(&segs, &[], 200, 140.0);
        let phases: Vec<_> = cycles.iter().map(|c| c.phase).collect();
        // Leading Accumulation, then sub-wave Accumulation [50,65],
        // then sub-wave Markup [65,90], then trailing Distribution.
        assert!(phases.contains(&WyckoffCyclePhase::Accumulation));
        assert!(phases.contains(&WyckoffCyclePhase::Markup));
        // Find the Markup tile and confirm it spans [W2, W5].
        let markup = cycles
            .iter()
            .find(|c| c.phase == WyckoffCyclePhase::Markup)
            .expect("markup tile expected");
        assert_eq!(markup.start_bar, 65);
        assert_eq!(markup.end_bar, 90);
        assert_eq!(markup.start_price, 102.0);
        assert_eq!(markup.end_price, 150.0);
    }

    #[test]
    fn subwave_bearish_abc_emits_distribution_then_markdown() {
        let waves = vec![
            (90, 150.0),   // X0 (= prev W5 top)
            (100, 130.0),  // A
            (110, 145.0),  // B (UTAD fakeout)
            (130, 110.0),  // C (capitulation low)
        ];
        let segs = vec![abc_with_waves(false, 90, 130, 150.0, 110.0, waves)];
        let cycles = detect_cycles_from_elliott(&segs, &[], 200, 115.0);
        // Skip the leading-fill Distribution; pick the sub-wave one
        // that starts exactly at X0 = 90.
        let dist = cycles
            .iter()
            .find(|c| {
                c.phase == WyckoffCyclePhase::Distribution && c.start_bar == 90
            })
            .expect("sub-wave distribution tile expected");
        let mkdn = cycles
            .iter()
            .find(|c| c.phase == WyckoffCyclePhase::Markdown)
            .expect("markdown tile expected");
        assert_eq!(dist.end_bar, 110); // X0 → B
        assert_eq!(mkdn.start_bar, 110); // B → C
        assert_eq!(mkdn.end_bar, 130);
    }

    #[test]
    fn leading_fill_capped_to_60_bars_for_giant_segments() {
        // A 1000-bar segment requests 1000/3 = 333 bars, cap kicks
        // in at 60.
        let segs = vec![motive(true, 1500, 2500, 100.0, 200.0)];
        let cycles = detect_cycles_from_elliott(&segs, &[], 3000, 180.0);
        assert!(cycles.len() >= 2);
        assert_eq!(cycles[0].phase, WyckoffCyclePhase::Accumulation);
        assert_eq!(cycles[0].start_bar, 1440); // 1500 - 60 (cap)
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
        // Elliott bounds are authoritative — confluent tile uses
        // Elliott's start/end, not the union. Event presence only
        // upgrades the source tag.
        assert_eq!(merged[0].start_bar, 15); // = Elliott start
        assert_eq!(merged[0].end_bar, 65);   // = Elliott end
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
    fn spring_boost_upgrades_markup_to_confluent() {
        // Markup tile [W2=50, W5=90]; Spring event at bar 53 (3 bars
        // after W2 = inside the 5-bar boost window).
        let cycles = vec![WyckoffCycle {
            phase: WyckoffCyclePhase::Markup,
            source: WyckoffCycleSource::Elliott,
            start_bar: 50,
            end_bar: 90,
            start_price: 100.0,
            end_price: 150.0,
            phase_high: 150.0,
            phase_low: 100.0,
            completed: true,
            source_pattern_id: Some("m-50".into()),
        }];
        let events = vec![WyckoffEvent {
            kind: WyckoffEventKind::Spring,
            variant: "bull",
            score: 1.0,
            bar_index: 53,
            reference_price: 99.0,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: String::new(),
        }];
        let boosted = boost_with_phase_c_events(cycles, &events, 5);
        assert_eq!(boosted.len(), 1);
        assert_eq!(boosted[0].source, WyckoffCycleSource::Confluent);
    }

    #[test]
    fn utad_boost_upgrades_markdown_to_confluent() {
        let cycles = vec![WyckoffCycle {
            phase: WyckoffCyclePhase::Markdown,
            source: WyckoffCycleSource::Elliott,
            start_bar: 100,
            end_bar: 140,
            start_price: 150.0,
            end_price: 110.0,
            phase_high: 150.0,
            phase_low: 110.0,
            completed: true,
            source_pattern_id: None,
        }];
        let events = vec![WyckoffEvent {
            kind: WyckoffEventKind::Utad,
            variant: "bear",
            score: 1.0,
            bar_index: 102,
            reference_price: 152.0,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: String::new(),
        }];
        let boosted = boost_with_phase_c_events(cycles, &events, 5);
        assert_eq!(boosted[0].source, WyckoffCycleSource::Confluent);
    }

    #[test]
    fn boost_skips_when_no_aligned_event() {
        let cycles = vec![WyckoffCycle {
            phase: WyckoffCyclePhase::Markup,
            source: WyckoffCycleSource::Elliott,
            start_bar: 50,
            end_bar: 90,
            start_price: 100.0,
            end_price: 150.0,
            phase_high: 150.0,
            phase_low: 100.0,
            completed: true,
            source_pattern_id: None,
        }];
        let events = vec![WyckoffEvent {
            kind: WyckoffEventKind::Spring,
            variant: "bull",
            score: 1.0,
            bar_index: 200, // far away
            reference_price: 99.0,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: String::new(),
        }];
        let boosted = boost_with_phase_c_events(cycles, &events, 5);
        assert_eq!(boosted[0].source, WyckoffCycleSource::Elliott);
    }

    #[test]
    fn slot_filter_drops_weak_events() {
        let mut events = vec![
            ev(WyckoffEventKind::Sc, 10, 100.0),
            ev(WyckoffEventKind::Bc, 60, 140.0),
        ];
        events[0].score = 0.6;
        events[1].score = 0.9;
        // BUG4 — when SC then BC fire without an explicit SoS in
        // between, the state machine inserts an INFERRED Markup
        // tile so the rotation alternates correctly. The Z0 path
        // (both events pass the score gate) now produces 3 tiles:
        // Accumulation [10..60], inferred Markup [pre-BC..60],
        // Distribution [60..200].
        let z0 = detect_cycles_for_slot(&events, &[], 0.55, 200, 100.0);
        let z3 = detect_cycles_for_slot(&events, &[], 0.85, 200, 100.0);
        assert_eq!(z0.len(), 3);
        assert!(z0.iter().any(|c| c.phase == WyckoffCyclePhase::Accumulation));
        assert!(z0.iter().any(|c| c.phase == WyckoffCyclePhase::Markup));
        assert!(z0.iter().any(|c| c.phase == WyckoffCyclePhase::Distribution));
        // High-bar gate drops SC; only BC survives → single
        // Distribution tile (no Accumulation prefix → no inferred
        // Markup either).
        assert_eq!(z3.len(), 1);
        assert_eq!(z3[0].phase, WyckoffCyclePhase::Distribution);
    }

    // BUG4 — when an Accumulation phase ends and BC fires without
    // an explicit SoS event between them, the state machine must
    // INFER a Markup tile instead of leaping straight from
    // Accumulation to Distribution. The inferred tile spans from
    // the lowest low in the Accumulation window to the BC bar so
    // the box covers the actual bullish leg.
    #[test]
    fn bc_after_accumulation_inserts_inferred_markup() {
        // Make a synthetic bar tape: bars 0..40 in [100, 110]
        // (accumulation range), then bars 40..70 climbing 110→140
        // (the bullish leg), bar 70 = BC.
        let mut bars = Vec::new();
        for i in 0..70 {
            let close = if i < 40 {
                100.0 + (i as f64 * 0.25) // small fluctuation 100..110
            } else {
                110.0 + ((i - 40) as f64) // rally 110→140
            };
            bars.push(make_bar(i, close - 0.5, close + 0.5, close - 1.0, close + 1.0));
        }
        let events = vec![
            ev_bull(WyckoffEventKind::Sc, 5, 100.0),
            ev_bear(WyckoffEventKind::Bc, 70, 145.0),
        ];
        let cycles = detect_cycles(&events, &bars, 100, 145.0);
        // Expect: Accumulation, inferred Markup, Distribution.
        let phases: Vec<_> = cycles.iter().map(|c| c.phase).collect();
        assert!(
            phases.contains(&WyckoffCyclePhase::Accumulation),
            "Accumulation tile missing: {phases:?}"
        );
        assert!(
            phases.contains(&WyckoffCyclePhase::Markup),
            "inferred Markup tile missing: {phases:?}"
        );
        assert!(
            phases.contains(&WyckoffCyclePhase::Distribution),
            "Distribution tile missing: {phases:?}"
        );
        // Inferred Markup must end at the BC bar.
        let markup = cycles
            .iter()
            .find(|c| c.phase == WyckoffCyclePhase::Markup)
            .unwrap();
        assert_eq!(markup.end_bar, 70, "inferred markup must end at BC");
        // And start somewhere INSIDE the accumulation window (not
        // at bar 0, not 1 bar before BC).
        assert!(markup.start_bar > 0 && markup.start_bar < 70);
    }

    // Helper: synthetic bar with the v2 Bar shape used by the
    // wyckoff crate (matches qtss_domain::v2::bar::Bar fixture).
    fn make_bar(idx: usize, open: f64, close: f64, low: f64, high: f64) -> Bar {
        use chrono::TimeZone;
        use rust_decimal::prelude::FromPrimitive;
        use qtss_domain::v2::instrument::{
            AssetClass, Instrument, SessionCalendar, Venue,
        };
        use qtss_domain::v2::timeframe::Timeframe;
        Bar {
            instrument: Instrument {
                venue: Venue::Binance,
                asset_class: AssetClass::CryptoFutures,
                symbol: "TESTUSDT".into(),
                quote_ccy: "USDT".into(),
                tick_size: rust_decimal::Decimal::from_f64(0.01).unwrap(),
                lot_size: rust_decimal::Decimal::from_f64(0.00001).unwrap(),
                session: SessionCalendar::binance_24x7(),
            },
            timeframe: Timeframe::H1,
            open_time: chrono::Utc
                .timestamp_opt(idx as i64 * 3600, 0)
                .unwrap(),
            open: rust_decimal::Decimal::from_f64(open).unwrap(),
            high: rust_decimal::Decimal::from_f64(high).unwrap(),
            low: rust_decimal::Decimal::from_f64(low).unwrap(),
            close: rust_decimal::Decimal::from_f64(close).unwrap(),
            volume: rust_decimal::Decimal::from(1000),
            closed: true,
        }
    }

    fn ev_bull(kind: WyckoffEventKind, bar_index: usize, price: f64) -> WyckoffEvent {
        WyckoffEvent {
            kind,
            variant: "bull",
            score: 1.0,
            bar_index,
            reference_price: price,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: String::new(),
        }
    }

    fn ev_bear(kind: WyckoffEventKind, bar_index: usize, price: f64) -> WyckoffEvent {
        WyckoffEvent {
            kind,
            variant: "bear",
            score: 1.0,
            bar_index,
            reference_price: price,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: String::new(),
        }
    }

    // BUG3 — three Accumulation tiles stacked on the same Z-level
    // must collapse to one spanning tile after dedupe.
    #[test]
    fn dedupe_collapses_three_accumulation_tiles() {
        let make = |source, start: usize, end: usize| WyckoffCycle {
            phase: WyckoffCyclePhase::Accumulation,
            source,
            start_bar: start,
            end_bar: end,
            start_price: 100.0,
            end_price: 110.0,
            phase_high: 115.0,
            phase_low: 95.0,
            completed: true,
            source_pattern_id: None,
        };
        let cycles = vec![
            make(WyckoffCycleSource::Event, 100, 150),
            make(WyckoffCycleSource::Elliott, 140, 180),
            make(WyckoffCycleSource::Event, 170, 200),
        ];
        let out = dedupe_consecutive_same_phase(cycles);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].start_bar, 100);
        assert_eq!(out[0].end_bar, 200);
        // Highest-priority source wins (Elliott > Event).
        assert_eq!(out[0].source, WyckoffCycleSource::Elliott);
    }

    // Distinct phases must NOT be collapsed even when adjacent.
    #[test]
    fn dedupe_keeps_distinct_phases_separate() {
        let cycles = vec![
            WyckoffCycle {
                phase: WyckoffCyclePhase::Accumulation,
                source: WyckoffCycleSource::Elliott,
                start_bar: 0,
                end_bar: 50,
                start_price: 100.0,
                end_price: 110.0,
                phase_high: 110.0,
                phase_low: 95.0,
                completed: true,
                source_pattern_id: None,
            },
            WyckoffCycle {
                phase: WyckoffCyclePhase::Markup,
                source: WyckoffCycleSource::Elliott,
                start_bar: 50,
                end_bar: 120,
                start_price: 110.0,
                end_price: 200.0,
                phase_high: 205.0,
                phase_low: 108.0,
                completed: true,
                source_pattern_id: None,
            },
            WyckoffCycle {
                phase: WyckoffCyclePhase::Accumulation,
                source: WyckoffCycleSource::Event,
                start_bar: 200,
                end_bar: 240,
                start_price: 150.0,
                end_price: 155.0,
                phase_high: 158.0,
                phase_low: 148.0,
                completed: true,
                source_pattern_id: None,
            },
        ];
        let out = dedupe_consecutive_same_phase(cycles);
        // Two non-adjacent Accumulations + one Markup — all preserved.
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].phase, WyckoffCyclePhase::Accumulation);
        assert_eq!(out[1].phase, WyckoffCyclePhase::Markup);
        assert_eq!(out[2].phase, WyckoffCyclePhase::Accumulation);
    }

    // BUG3 round 2 — overlapping different-phase tiles must collapse
    // to a single non-overlapping timeline. Higher-priority source
    // (Confluent > Elliott > Event) wins the contested bars; lower-
    // priority tiles compress into the surrounding gaps.
    #[test]
    fn enforce_non_overlap_clips_lower_priority_inside_higher() {
        let make = |source, phase, start: usize, end: usize| WyckoffCycle {
            phase,
            source,
            start_bar: start,
            end_bar: end,
            start_price: 100.0,
            end_price: 110.0,
            phase_high: 115.0,
            phase_low: 95.0,
            completed: true,
            source_pattern_id: None,
        };
        // Event tile spans [0, 200] — long but lowest priority.
        // Elliott tile [50, 150] — different phase, mid priority.
        // Result: Elliott wins [50, 150]; Event survives only on
        // the wings [0, 50] and [150, 200].
        let cycles = vec![
            make(WyckoffCycleSource::Event, WyckoffCyclePhase::Distribution, 0, 200),
            make(WyckoffCycleSource::Elliott, WyckoffCyclePhase::Markdown, 50, 150),
        ];
        let out = enforce_non_overlap(cycles);
        assert_eq!(out.len(), 3);
        // Sorted by start_bar.
        assert_eq!(out[0].source, WyckoffCycleSource::Event);
        assert_eq!(out[0].start_bar, 0);
        assert_eq!(out[0].end_bar, 50);
        assert_eq!(out[1].source, WyckoffCycleSource::Elliott);
        assert_eq!(out[1].start_bar, 50);
        assert_eq!(out[1].end_bar, 150);
        assert_eq!(out[2].source, WyckoffCycleSource::Event);
        assert_eq!(out[2].start_bar, 150);
        assert_eq!(out[2].end_bar, 200);
    }

    // Same-priority overlap: longer tile wins. Three event tiles,
    // the longest claims its full span; the shorter ones compress.
    #[test]
    fn enforce_non_overlap_uses_length_as_tiebreaker() {
        let make = |phase, start: usize, end: usize| WyckoffCycle {
            phase,
            source: WyckoffCycleSource::Event,
            start_bar: start,
            end_bar: end,
            start_price: 100.0,
            end_price: 110.0,
            phase_high: 115.0,
            phase_low: 95.0,
            completed: true,
            source_pattern_id: None,
        };
        let cycles = vec![
            make(WyckoffCyclePhase::Markup, 10, 60),         // len 50
            make(WyckoffCyclePhase::Distribution, 0, 100),   // len 100 (winner)
            make(WyckoffCyclePhase::Accumulation, 70, 90),   // len 20
        ];
        let out = enforce_non_overlap(cycles);
        // Longest Distribution claims [0, 100] entirely. Markup
        // [10, 60] is fully inside → dropped. Accumulation [70, 90]
        // is fully inside → dropped.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].phase, WyckoffCyclePhase::Distribution);
        assert_eq!(out[0].start_bar, 0);
        assert_eq!(out[0].end_bar, 100);
    }

    // Non-overlapping tiles pass through untouched.
    #[test]
    fn enforce_non_overlap_preserves_disjoint_tiles() {
        let make = |phase, start: usize, end: usize| WyckoffCycle {
            phase,
            source: WyckoffCycleSource::Elliott,
            start_bar: start,
            end_bar: end,
            start_price: 100.0,
            end_price: 110.0,
            phase_high: 115.0,
            phase_low: 95.0,
            completed: true,
            source_pattern_id: None,
        };
        let cycles = vec![
            make(WyckoffCyclePhase::Accumulation, 0, 50),
            make(WyckoffCyclePhase::Markup, 50, 100),
            make(WyckoffCyclePhase::Distribution, 100, 150),
        ];
        let out = enforce_non_overlap(cycles);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].phase, WyckoffCyclePhase::Accumulation);
        assert_eq!(out[1].phase, WyckoffCyclePhase::Markup);
        assert_eq!(out[2].phase, WyckoffCyclePhase::Distribution);
    }

    // Confluent must beat both Elliott and Event in source priority.
    #[test]
    fn dedupe_promotes_to_confluent_when_present() {
        let make = |source, start: usize, end: usize| WyckoffCycle {
            phase: WyckoffCyclePhase::Markup,
            source,
            start_bar: start,
            end_bar: end,
            start_price: 100.0,
            end_price: 200.0,
            phase_high: 200.0,
            phase_low: 100.0,
            completed: true,
            source_pattern_id: None,
        };
        let cycles = vec![
            make(WyckoffCycleSource::Event, 0, 50),
            make(WyckoffCycleSource::Confluent, 40, 90),
            make(WyckoffCycleSource::Elliott, 80, 120),
        ];
        let out = dedupe_consecutive_same_phase(cycles);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, WyckoffCycleSource::Confluent);
    }
}
