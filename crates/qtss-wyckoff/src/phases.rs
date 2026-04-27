//! Canonical Wyckoff schematic phase classifier (Phase A → E).
//!
//! Maps a stream of Wyckoff events plus the underlying bar tape to
//! a sequence of `WyckoffSchematicSpan`s — one per phase — that the
//! GUI overlays as bracketed boxes on the chart, and the cycle
//! pipeline cross-references against its 4-phase rotation.
//!
//! Anchoring rules follow the StockCharts ChartSchool tutorial and
//! the Trading Wyckoff phase guide (URLs in
//! `docs/WYCKOFF_METHOD.md`):
//!
//! ## Accumulation
//! ```text
//! Phase A (Stopping action)   ← PS  ─ SC  ─ AR  ─ ST
//! Phase B (Building cause)    ← (last ST of A) → Spring
//! Phase C (Test of supply)    ← Spring + Test of Spring
//! Phase D (Move out of TR)    ← SOS  ─ LPS  ─ BU/JAC
//! Phase E (Markup)            ← BU/JAC → next markup leg
//! ```
//!
//! ## Distribution (mirror)
//! ```text
//! Phase A   ← PSY ─ BC  ─ AR  ─ ST
//! Phase B   ← (last ST) → UTAD
//! Phase C   ← UTAD + Test of UTAD
//! Phase D   ← SOW ─ LPSY ─ Breakdown
//! Phase E   ← Breakdown → Markdown leg
//! ```
//!
//! When a phase's anchor event is missing the classifier falls back
//! to the surrounding phases' boundaries (Phase B is whatever
//! survives between A and C, etc). Phase E is bounded by the next
//! direction reversal — if the markup runs all the way until the
//! next BC event, Phase E ends there.

use crate::cycle::{
    detect_cycles, detect_cycles_from_elliott, dedupe_consecutive_same_phase,
    enforce_non_overlap_with_bars, ElliottSegment, WyckoffCycle,
};
use crate::event::{WyckoffEvent, WyckoffEventKind};
use qtss_domain::v2::bar::Bar;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffSchematicPhase {
    /// Stopping action — PS/PSY through ST.
    A,
    /// Building cause — sideways absorption inside the TR.
    B,
    /// Test of supply / demand — Spring or UTAD plus its retest.
    C,
    /// Move out of the trading range — SOS / SOW signature events.
    D,
    /// Markup / Markdown — the trend leg that follows BU / BO.
    E,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffSchematicDirection {
    Accumulation,
    Distribution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WyckoffSchematicSpan {
    pub phase: WyckoffSchematicPhase,
    pub direction: WyckoffSchematicDirection,
    /// Inclusive bar where the phase opens.
    pub start_bar: usize,
    /// Inclusive bar where the phase closes (or last fed bar when
    /// still active).
    pub end_bar: usize,
    /// Highest HIGH across the phase span.
    pub phase_high: f64,
    /// Lowest LOW across the phase span.
    pub phase_low: f64,
    /// Event bar indices that anchored this phase. Used by the GUI
    /// to draw the bracket markers above / below the phase box.
    pub anchor_events: Vec<EventAnchor>,
    /// `true` once the next phase opens; `false` while still
    /// active at the tape head.
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventAnchor {
    pub kind: String,
    pub bar_index: usize,
    pub price: f64,
}

/// Full Wyckoff view — both layers in one struct so callers (the
/// engine writer, the GUI, the API) can render the macro 4-phase
/// rotation AND the canonical 5-phase schematic side-by-side.
///
/// `macro_cycles` carries the four-phase macro tiles
///   (Accumulation → Markup → Distribution → Markdown)
/// computed from the merged Event + Elliott pipeline (see `cycle.rs`).
///
/// `schematic_phases` carries the canonical 5-phase Wyckoff
/// sub-phases (Phase A → Phase E) per Accumulation / Distribution
/// side, anchored to the events documented in
/// `docs/WYCKOFF_METHOD.md`.
///
/// The two layers complement each other: the macro tiles tell the
/// trader WHICH side of the cycle they're in (basing? rallying?
/// topping? declining?), and the schematic phases tell them WHERE
/// inside that side they are (still in stopping action? past the
/// Spring? ready to ride the markup leg?).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WyckoffPhaseView {
    pub macro_cycles: Vec<WyckoffCycle>,
    pub schematic_phases: Vec<WyckoffSchematicSpan>,
}

/// Compute BOTH layers from the same input. Single entry point so
/// callers don't have to worry about which crate function to call
/// first or whether the macro and schematic sides agree.
///
/// Macro pipeline (re-uses cycle.rs):
///   1. event-driven cycles (`detect_cycles`)
///   2. elliott-anchored cycles (`detect_cycles_from_elliott`)
///   3. enforce_non_overlap to clip overlapping different-phase tiles
///   4. dedupe_consecutive_same_phase to fuse adjacent same-phase tiles
///
/// Schematic pipeline (`classify_schematic_phases` below):
///   1. sort events by bar
///   2. split by direction (bull = Accumulation side, bear = Distribution)
///   3. anchor each phase A-E to its canonical event(s)
///
/// Returns both as a single struct ready to serialize.
pub fn compute_wyckoff_view(
    events: &[WyckoffEvent],
    elliott_segments: &[ElliottSegment],
    bars: &[Bar],
    tape_end_bar: usize,
    tape_end_price: f64,
) -> WyckoffPhaseView {
    // Macro layer.
    let event_cycles =
        detect_cycles(events, bars, tape_end_bar, tape_end_price);
    let elliott_cycles = detect_cycles_from_elliott(
        elliott_segments,
        bars,
        tape_end_bar,
        tape_end_price,
    );
    let merged: Vec<_> =
        event_cycles.into_iter().chain(elliott_cycles).collect();
    let merged = enforce_non_overlap_with_bars(merged, bars);
    let merged = dedupe_consecutive_same_phase(merged);

    // Schematic layer.
    let schematic = classify_schematic_phases(events, bars, tape_end_bar);

    WyckoffPhaseView {
        macro_cycles: merged,
        schematic_phases: schematic,
    }
}

/// Top-level entry: sort events, split by direction, run the
/// schematic classifier on each side, then enforce SIDE MUTEX so
/// Acc-side and Dist-side phases don't render on top of each
/// other. Wyckoff doctrine: at any bar the market is in EITHER an
/// accumulation OR a distribution schematic, never both.
///
/// Side mutex algorithm (2026-04-27 chart audit fix):
/// 1. Run the classifier independently on both sides → get raw
///    spans tagged Acc / Dist.
/// 2. For each span S, if an opposite-direction span O overlaps
///    S's range AND O's anchor (event bar that triggered Phase A
///    of that side) sits inside S, clip S to end at O's start.
/// 3. Phase E spans are particularly aggressive about this — once
///    the OPPOSITE side's Phase A starts, the current side's
///    Phase E (the trend leg) is officially terminated.
pub fn classify_schematic_phases(
    events: &[WyckoffEvent],
    bars: &[Bar],
    tape_end_bar: usize,
) -> Vec<WyckoffSchematicSpan> {
    if events.is_empty() {
        return Vec::new();
    }
    let mut sorted: Vec<&WyckoffEvent> = events.iter().collect();
    sorted.sort_by_key(|e| e.bar_index);

    let bull: Vec<&WyckoffEvent> = sorted
        .iter()
        .filter(|e| e.variant == "bull")
        .copied()
        .collect();
    let bear: Vec<&WyckoffEvent> = sorted
        .iter()
        .filter(|e| e.variant == "bear")
        .copied()
        .collect();

    let mut acc_spans = classify_side(
        &bull,
        bars,
        tape_end_bar,
        WyckoffSchematicDirection::Accumulation,
    );
    let mut dist_spans = classify_side(
        &bear,
        bars,
        tape_end_bar,
        WyckoffSchematicDirection::Distribution,
    );

    // Side mutex — find each side's Phase A start anchor and use
    // it to clip the OTHER side's overlapping spans.
    let acc_phase_a_starts: Vec<usize> = acc_spans
        .iter()
        .filter(|s| s.phase == WyckoffSchematicPhase::A)
        .map(|s| s.start_bar)
        .collect();
    let dist_phase_a_starts: Vec<usize> = dist_spans
        .iter()
        .filter(|s| s.phase == WyckoffSchematicPhase::A)
        .map(|s| s.start_bar)
        .collect();
    clip_spans_by_opposite_anchors(&mut acc_spans, &dist_phase_a_starts);
    clip_spans_by_opposite_anchors(&mut dist_spans, &acc_phase_a_starts);

    let mut out = Vec::with_capacity(acc_spans.len() + dist_spans.len());
    // Single-bar spans (Phase C with Spring but no Test) survive —
    // they're a valid Wyckoff phase even at one bar. Only drop
    // spans that got CLIPPED to negative width by side mutex.
    out.extend(acc_spans.into_iter().filter(|s| s.end_bar >= s.start_bar));
    out.extend(dist_spans.into_iter().filter(|s| s.end_bar >= s.start_bar));
    out.sort_by_key(|s| (s.start_bar, schematic_phase_rank(s.phase)));

    // 2026-04-28 — strict same-side phase sequencing. Wyckoff
    // doctrine: A → B → C → D → E never overlaps within one
    // schematic. Inputs to classify_side already enforce this by
    // construction (each phase's bounds are derived from the
    // surrounding phases' anchors), but we double-check here:
    // walk per-side, when phase X.start lies inside phase Y where
    // Y.end > X.start AND rank(Y) < rank(X), clip Y.end to
    // X.start - 1 so the prior phase yields to the next one
    // taking over.
    enforce_intra_side_sequencing(&mut out);
    out
}

fn enforce_intra_side_sequencing(spans: &mut Vec<WyckoffSchematicSpan>) {
    // For each side, ensure A → B → C → D → E is strictly
    // sequential. When a higher-ranked phase starts inside a
    // lower-ranked one, the lower-ranked one yields.
    let dir_rank = |d: WyckoffSchematicDirection| -> u8 {
        match d {
            WyckoffSchematicDirection::Accumulation => 0,
            WyckoffSchematicDirection::Distribution => 1,
        }
    };
    spans.sort_by_key(|s| {
        (dir_rank(s.direction), s.start_bar, schematic_phase_rank(s.phase))
    });
    for i in 0..spans.len() {
        let s_i_dir = spans[i].direction;
        let s_i_rank = schematic_phase_rank(spans[i].phase);
        let s_i_start = spans[i].start_bar;
        for j in 0..i {
            if spans[j].direction != s_i_dir {
                continue;
            }
            let s_j_rank = schematic_phase_rank(spans[j].phase);
            if s_j_rank < s_i_rank
                && spans[j].end_bar >= s_i_start
                && spans[j].start_bar < s_i_start
            {
                spans[j].end_bar = s_i_start.saturating_sub(1);
            }
        }
    }
    spans.retain(|s| s.end_bar >= s.start_bar);
}

/// Clip spans against opposite-direction Phase A anchors. When an
/// opposite-side Phase A starts INSIDE a span, the span ends at
/// (anchor - 1). This is the side mutex enforcement — Wyckoff
/// doctrine has each schematic playing out in its own time window;
/// concurrent schematics are an artefact of the per-side
/// classifier running independently.
fn clip_spans_by_opposite_anchors(
    spans: &mut Vec<WyckoffSchematicSpan>,
    anchors: &[usize],
) {
    for span in spans.iter_mut() {
        for &a in anchors {
            if a > span.start_bar && a < span.end_bar {
                span.end_bar = a.saturating_sub(1).max(span.start_bar);
            }
        }
    }
}

fn schematic_phase_rank(p: WyckoffSchematicPhase) -> u8 {
    match p {
        WyckoffSchematicPhase::A => 0,
        WyckoffSchematicPhase::B => 1,
        WyckoffSchematicPhase::C => 2,
        WyckoffSchematicPhase::D => 3,
        WyckoffSchematicPhase::E => 4,
    }
}

/// Classify a single direction (Accumulation OR Distribution).
///
/// Algorithm — anchor-driven:
///   1. Walk events forward, gathering candidate anchors per phase.
///   2. Phase A: starts at the first PS / PSY (or, when absent, the
///      first SC / BC); ends at the LAST ST before the first
///      Spring / UTAD.
///   3. Phase B: from end of Phase A to the bar BEFORE the first
///      Spring / UTAD.
///   4. Phase C: the Spring / UTAD bar plus the bar of its Test
///      (when present).
///   5. Phase D: from the first SOS / SOW after Phase C through
///      the last LPS / LPSY (or BU / BO).
///   6. Phase E: from the BU / BO breakout through `tape_end_bar`
///      (or earlier if a same-direction A re-enters the picture).
///
/// Each phase emits a span only when its trigger anchor is
/// present — Phase B can exist without explicit Phase B-only
/// events because its boundaries are the surrounding phases.
fn classify_side(
    events: &[&WyckoffEvent],
    bars: &[Bar],
    tape_end_bar: usize,
    direction: WyckoffSchematicDirection,
) -> Vec<WyckoffSchematicSpan> {
    use WyckoffEventKind::*;
    let bull = matches!(direction, WyckoffSchematicDirection::Accumulation);
    if events.is_empty() {
        return Vec::new();
    }
    // Collect anchor bars per kind.
    let by_kind = |k: WyckoffEventKind| -> Vec<&WyckoffEvent> {
        events.iter().copied().filter(|e| e.kind == k).collect()
    };
    let ps = by_kind(Ps); // works for both PS (bull) and PSY (bear)
    let sc_or_bc = by_kind(if bull { Sc } else { Bc });
    let ar = by_kind(Ar);
    let st = by_kind(St);
    let spring_or_utad =
        by_kind(if bull { Spring } else { Utad });
    let test_events = by_kind(Test);
    let sos_or_sow = by_kind(if bull { Sos } else { Sow });
    let lps = by_kind(Lps);
    let bu = by_kind(Bu);

    let phase_a_anchor_start = ps
        .first()
        .map(|e| e.bar_index)
        .or_else(|| sc_or_bc.first().map(|e| e.bar_index));
    let phase_a_anchor_end = st
        .iter()
        .filter(|e| {
            spring_or_utad
                .first()
                .map(|sp| e.bar_index < sp.bar_index)
                .unwrap_or(true)
        })
        .last()
        .map(|e| e.bar_index)
        .or_else(|| ar.first().map(|e| e.bar_index));

    let phase_c_start = spring_or_utad.first().map(|e| e.bar_index);
    let phase_c_end = test_events
        .iter()
        .filter(|e| {
            phase_c_start
                .map(|c| e.bar_index >= c)
                .unwrap_or(false)
        })
        .last()
        .map(|e| e.bar_index)
        .or(phase_c_start);

    let phase_d_start = sos_or_sow
        .iter()
        .find(|e| {
            phase_c_end
                .map(|c| e.bar_index > c)
                .unwrap_or(true)
        })
        .map(|e| e.bar_index);
    let phase_d_end = bu
        .iter()
        .filter(|e| {
            phase_d_start
                .map(|d| e.bar_index >= d)
                .unwrap_or(false)
        })
        .last()
        .map(|e| e.bar_index)
        .or_else(|| {
            lps.iter()
                .filter(|e| {
                    phase_d_start
                        .map(|d| e.bar_index >= d)
                        .unwrap_or(false)
                })
                .last()
                .map(|e| e.bar_index)
        })
        .or(phase_d_start);

    let phase_e_start = phase_d_end;
    // BUG (2026-04-27 chart audit) — Phase E used to extend to
    // `tape_end_bar` for every detected schematic, painting a 1000+
    // bar dashed box across the entire chart. Doctrine says Phase E
    // is the markup/markdown leg that follows the breakout; once
    // the OPPOSITE side's Phase A starts (Distribution begins after
    // Markup tops), the current Phase E ends.
    //
    // We approximate this by capping Phase E at:
    //   - the bar where the opposite direction's PS/PSY or SC/BC
    //     event fires (clear cycle reversal), OR
    //   - phase_d_end + 3 × max(phase_d_duration, 30) bars (so a
    //     thin Phase D doesn't make Phase E run forever), OR
    //   - tape_end_bar (whichever is smallest).
    let opposite_kind_first_anchor: Option<usize> = {
        // The classifier runs once per direction; events vector
        // here is already filtered to ONE side. The OPPOSITE side's
        // first stopping-action anchor lives in the FULL events
        // list, but we don't have that here. Instead we use the
        // tape_end_bar bound + lookforward cap.
        None
    };
    let _ = opposite_kind_first_anchor;
    let phase_e_lookforward = phase_d_start
        .zip(phase_d_end)
        .map(|(s, e)| (e.saturating_sub(s)).max(30) * 3)
        .unwrap_or(150);
    let phase_e_end = phase_e_start
        .map(|s| (s + phase_e_lookforward).min(tape_end_bar))
        .unwrap_or(tape_end_bar);

    let mut out = Vec::new();
    let mk = |phase: WyckoffSchematicPhase,
              start: usize,
              end: usize,
              anchors: Vec<EventAnchor>|
     -> WyckoffSchematicSpan {
        let (hi, lo) = bar_bounds(bars, start, end);
        WyckoffSchematicSpan {
            phase,
            direction,
            start_bar: start,
            end_bar: end,
            phase_high: hi,
            phase_low: lo,
            anchor_events: anchors,
            completed: end < tape_end_bar,
        }
    };
    let to_anchor = |e: &WyckoffEvent| EventAnchor {
        kind: format!("{:?}", e.kind).to_lowercase(),
        bar_index: e.bar_index,
        price: e.reference_price,
    };

    // Phase A
    if let (Some(s), Some(e)) = (phase_a_anchor_start, phase_a_anchor_end) {
        if e >= s {
            let mut anchors: Vec<EventAnchor> = Vec::new();
            anchors.extend(ps.iter().map(|x| to_anchor(*x)));
            anchors.extend(sc_or_bc.iter().map(|x| to_anchor(*x)));
            anchors.extend(ar.iter().map(|x| to_anchor(*x)));
            anchors.extend(
                st.iter()
                    .filter(|e| e.bar_index <= phase_a_anchor_end.unwrap())
                    .map(|x| to_anchor(*x)),
            );
            out.push(mk(WyckoffSchematicPhase::A, s, e, anchors));
        }
    }

    // Phase B — between A's end and Spring/UTAD start.
    if let (Some(b_start), Some(b_end)) = (
        phase_a_anchor_end.map(|x| x.saturating_add(1)),
        phase_c_start.map(|x| x.saturating_sub(1)),
    ) {
        if b_end > b_start {
            // Anchor B with any STs that fall in this window.
            let anchors: Vec<EventAnchor> = st
                .iter()
                .filter(|e| {
                    e.bar_index > phase_a_anchor_end.unwrap()
                        && e.bar_index < phase_c_start.unwrap()
                })
                .map(|x| to_anchor(*x))
                .collect();
            out.push(mk(WyckoffSchematicPhase::B, b_start, b_end, anchors));
        }
    }

    // Phase C — Spring/UTAD plus its test.
    if let (Some(s), Some(e)) = (phase_c_start, phase_c_end) {
        let mut anchors: Vec<EventAnchor> = Vec::new();
        anchors.extend(spring_or_utad.iter().map(|x| to_anchor(*x)));
        anchors.extend(
            test_events
                .iter()
                .filter(|t| t.bar_index >= s && t.bar_index <= e)
                .map(|x| to_anchor(*x)),
        );
        out.push(mk(WyckoffSchematicPhase::C, s, e, anchors));
    }

    // Phase D — SOS through LPS/BU.
    if let (Some(s), Some(e)) = (phase_d_start, phase_d_end) {
        let mut anchors: Vec<EventAnchor> = Vec::new();
        anchors.extend(
            sos_or_sow
                .iter()
                .filter(|x| x.bar_index >= s && x.bar_index <= e)
                .map(|x| to_anchor(*x)),
        );
        anchors.extend(
            lps.iter()
                .filter(|x| x.bar_index >= s && x.bar_index <= e)
                .map(|x| to_anchor(*x)),
        );
        anchors.extend(
            bu.iter()
                .filter(|x| x.bar_index >= s && x.bar_index <= e)
                .map(|x| to_anchor(*x)),
        );
        out.push(mk(WyckoffSchematicPhase::D, s, e, anchors));
    }

    // Phase E — markup / markdown leg from D's end to tape head.
    if let Some(s) = phase_e_start {
        if phase_e_end > s {
            out.push(mk(
                WyckoffSchematicPhase::E,
                s,
                phase_e_end,
                Vec::new(),
            ));
        }
    }

    out
}

fn bar_bounds(bars: &[Bar], start: usize, end: usize) -> (f64, f64) {
    use rust_decimal::prelude::ToPrimitive;
    if bars.is_empty() {
        return (0.0, 0.0);
    }
    let s = start.min(bars.len() - 1);
    let e = end.min(bars.len() - 1).max(s);
    let mut hi = f64::NEG_INFINITY;
    let mut lo = f64::INFINITY;
    for b in &bars[s..=e] {
        let h = b.high.to_f64().unwrap_or(0.0);
        let l = b.low.to_f64().unwrap_or(0.0);
        if h > hi {
            hi = h;
        }
        if l < lo {
            lo = l;
        }
    }
    if !hi.is_finite() {
        hi = 0.0;
    }
    if !lo.is_finite() {
        lo = 0.0;
    }
    (hi, lo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::WyckoffEvent;
    use chrono::TimeZone;
    use qtss_domain::v2::instrument::{
        AssetClass, Instrument, SessionCalendar, Venue,
    };
    use qtss_domain::v2::timeframe::Timeframe;
    use rust_decimal::prelude::FromPrimitive;

    fn synth_bar(idx: usize, mid: f64) -> Bar {
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
            open: rust_decimal::Decimal::from_f64(mid).unwrap(),
            high: rust_decimal::Decimal::from_f64(mid + 1.0).unwrap(),
            low: rust_decimal::Decimal::from_f64(mid - 1.0).unwrap(),
            close: rust_decimal::Decimal::from_f64(mid).unwrap(),
            volume: rust_decimal::Decimal::from(1000),
            closed: true,
        }
    }

    fn ev(
        kind: WyckoffEventKind,
        variant: &'static str,
        bar_index: usize,
        price: f64,
    ) -> WyckoffEvent {
        WyckoffEvent {
            kind,
            variant,
            score: 1.0,
            bar_index,
            reference_price: price,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: String::new(),
        }
    }

    /// Canonical accumulation sequence: PS → SC → AR → ST → Spring
    /// → Test → SOS → LPS → BU. The classifier should emit all
    /// five phases A..E in order.
    #[test]
    fn full_accumulation_sequence_emits_all_five_phases() {
        let bars: Vec<Bar> =
            (0..200).map(|i| synth_bar(i, 100.0 + i as f64 * 0.1)).collect();
        let events = vec![
            ev(WyckoffEventKind::Ps, "bull", 10, 99.0),
            ev(WyckoffEventKind::Sc, "bull", 15, 95.0),
            ev(WyckoffEventKind::Ar, "bull", 25, 105.0),
            ev(WyckoffEventKind::St, "bull", 40, 96.0),
            ev(WyckoffEventKind::Spring, "bull", 70, 93.0),
            ev(WyckoffEventKind::Test, "bull", 80, 94.0),
            ev(WyckoffEventKind::Sos, "bull", 100, 110.0),
            ev(WyckoffEventKind::Lps, "bull", 120, 108.0),
            ev(WyckoffEventKind::Bu, "bull", 140, 109.0),
        ];
        let phases = classify_schematic_phases(&events, &bars, 199);
        let names: Vec<_> = phases.iter().map(|p| p.phase).collect();
        assert!(
            names.contains(&WyckoffSchematicPhase::A),
            "Phase A missing: {names:?}"
        );
        assert!(
            names.contains(&WyckoffSchematicPhase::B),
            "Phase B missing: {names:?}"
        );
        assert!(
            names.contains(&WyckoffSchematicPhase::C),
            "Phase C missing: {names:?}"
        );
        assert!(
            names.contains(&WyckoffSchematicPhase::D),
            "Phase D missing: {names:?}"
        );
        assert!(
            names.contains(&WyckoffSchematicPhase::E),
            "Phase E missing: {names:?}"
        );
    }

    /// Distribution sequence emits five phases on the bear side.
    #[test]
    fn full_distribution_sequence_emits_all_five_phases() {
        let bars: Vec<Bar> =
            (0..200).map(|i| synth_bar(i, 200.0 - i as f64 * 0.1)).collect();
        let events = vec![
            ev(WyckoffEventKind::Ps, "bear", 10, 201.0),
            ev(WyckoffEventKind::Bc, "bear", 15, 205.0),
            ev(WyckoffEventKind::Ar, "bear", 25, 195.0),
            ev(WyckoffEventKind::St, "bear", 40, 204.0),
            ev(WyckoffEventKind::Utad, "bear", 70, 207.0),
            ev(WyckoffEventKind::Test, "bear", 80, 206.0),
            ev(WyckoffEventKind::Sow, "bear", 100, 190.0),
            ev(WyckoffEventKind::Lps, "bear", 120, 192.0),
            ev(WyckoffEventKind::Bu, "bear", 140, 191.0),
        ];
        let phases = classify_schematic_phases(&events, &bars, 199);
        let acc_phases: Vec<_> = phases
            .iter()
            .filter(|p| {
                p.direction == WyckoffSchematicDirection::Distribution
            })
            .map(|p| p.phase)
            .collect();
        assert_eq!(acc_phases.len(), 5, "phases: {acc_phases:?}");
    }

    /// Sparse events (missing Phase A anchors) — classifier still
    /// emits Phase C / D / E from the Spring / SOS markers alone.
    #[test]
    fn sparse_event_set_still_emits_phase_c_through_e() {
        let bars: Vec<Bar> =
            (0..200).map(|i| synth_bar(i, 100.0)).collect();
        let events = vec![
            ev(WyckoffEventKind::Spring, "bull", 70, 93.0),
            ev(WyckoffEventKind::Sos, "bull", 100, 110.0),
            ev(WyckoffEventKind::Bu, "bull", 140, 109.0),
        ];
        let phases = classify_schematic_phases(&events, &bars, 199);
        let names: Vec<_> = phases.iter().map(|p| p.phase).collect();
        assert!(names.contains(&WyckoffSchematicPhase::C));
        assert!(names.contains(&WyckoffSchematicPhase::D));
        assert!(names.contains(&WyckoffSchematicPhase::E));
    }
}
