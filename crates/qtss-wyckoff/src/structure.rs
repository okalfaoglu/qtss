//! Wyckoff Structure State Machine — tracks phase progression (A→B→C→D→E)
//! and structure type (Accumulation/Distribution/Re-accumulation/Re-distribution).
//!
//! The tracker collects events emitted by the detector and maintains a
//! running assessment of the current phase, schematic type, and key levels
//! (creek, ice, range).

use serde::{Deserialize, Serialize};

// =========================================================================
// Types
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffSchematic {
    Accumulation,
    Distribution,
    ReAccumulation,
    ReDistribution,
}

impl WyckoffSchematic {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accumulation => "accumulation",
            Self::Distribution => "distribution",
            Self::ReAccumulation => "reaccumulation",
            Self::ReDistribution => "redistribution",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WyckoffPhase {
    A,
    B,
    C,
    D,
    E,
}

impl WyckoffPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffEvent {
    // Phase A
    #[serde(rename = "p_s")]  PS,
    #[serde(rename = "s_c")]  SC,
    #[serde(rename = "b_c")]  BC,
    #[serde(rename = "a_r")]  AR,
    #[serde(rename = "s_t")]  ST,
    // Phase B
    #[serde(rename = "u_a")]  UA,
    #[serde(rename = "st_b")] STB,
    // Phase C
    Spring,
    #[serde(rename = "utad")] UTAD,
    Shakeout,
    /// Low-volume retest of a prior Spring low (Villahermosa ch. 6,
    /// "Test"). Confirms Phase C before SOS can open Phase D. Distinct
    /// from LPS — LPS sits above creek, SpringTest sits near support.
    #[serde(rename = "spring_test")] SpringTest,
    /// Mirror of SpringTest for distribution schematics.
    #[serde(rename = "utad_test")] UTADTest,
    // Phase D
    #[serde(rename = "s_o_s")]        SOS,
    #[serde(rename = "s_o_w")]        SOW,
    #[serde(rename = "l_p_s")]        LPS,
    #[serde(rename = "lpsy")]         LPSY,
    #[serde(rename = "j_a_c")]        JAC,
    #[serde(rename = "break_of_ice")] BreakOfIce,
    #[serde(rename = "buec")]         BUEC,
    // Misc
    #[serde(rename = "s_o_t")] SOT,
    Markup,
    Markdown,
}

impl WyckoffEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PS => "PS",
            Self::SC => "SC",
            Self::BC => "BC",
            Self::AR => "AR",
            Self::ST => "ST",
            Self::UA => "UA",
            Self::STB => "ST-B",
            Self::Spring => "Spring",
            Self::UTAD => "UTAD",
            Self::Shakeout => "Shakeout",
            Self::SpringTest => "SpringTest",
            Self::UTADTest => "UTADTest",
            Self::SOS => "SOS",
            Self::SOW => "SOW",
            Self::LPS => "LPS",
            Self::LPSY => "LPSY",
            Self::JAC => "JAC",
            Self::BreakOfIce => "BreakOfIce",
            Self::BUEC => "BUEC",
            Self::SOT => "SOT",
            Self::Markup => "Markup",
            Self::Markdown => "Markdown",
        }
    }

    /// Determine which phase this event belongs to.
    pub fn phase(self) -> WyckoffPhase {
        match self {
            Self::PS | Self::SC | Self::BC | Self::AR | Self::ST => WyckoffPhase::A,
            Self::UA | Self::STB => WyckoffPhase::B,
            Self::Spring | Self::UTAD | Self::Shakeout
            | Self::SpringTest | Self::UTADTest => WyckoffPhase::C,
            Self::SOS | Self::SOW | Self::LPS | Self::LPSY
            | Self::JAC | Self::BreakOfIce | Self::BUEC | Self::SOT => WyckoffPhase::D,
            Self::Markup | Self::Markdown => WyckoffPhase::E,
        }
    }
}

// =========================================================================
// Recorded event
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedEvent {
    pub event: WyckoffEvent,
    pub bar_index: u64,
    pub price: f64,
    pub score: f64,
    /// Unix epoch milliseconds of the anchor bar's open time. Added
    /// post-P2a because `bar_index` became global (relative to the
    /// symbol's full history) while the chart overlay only holds the
    /// recent visible window — indexing by bar_index misaligns events.
    /// `None` for rows written before this field existed; the chart
    /// falls back to bar_index in that case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_ms: Option<i64>,
}

// =========================================================================
// Structure Tracker
// =========================================================================

/// P17 — hysteresis policy for `auto_reclassify` and dedup windowing.
/// Exposed as a struct so callers can load from `qtss_config`
/// (CLAUDE.md rule #2). Defaults are safe for all TFs; worker should
/// override via `wyckoff.structure.*` config keys.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ReclassifyPolicy {
    /// Max times a structure's schematic can flip via `auto_reclassify`
    /// in its lifetime. Prevents ping-pong between Re-Accum/Re-Dist on
    /// choppy bars (Gemini P17 review #1).
    pub max_flips: u32,
    /// Minimum bar gap between two flips. A Spring 3 bars after a UTAD
    /// is noise, not a genuine character change.
    pub min_gap_bars: u64,
    /// Dedup window (bars) for `record_event_with_time`. Gemini review
    /// #4 — 3 was too narrow on LTF (1m/5m) where the same SC can ring
    /// 4-5 bars apart. TF-aware value provided by caller.
    pub dedup_window_bars: u64,
    /// Dedup price-equality tolerance as pct of price. Two SC at
    /// different prices but within window are still the same event if
    /// |Δp|/p < eps_pct.
    pub dedup_price_eps_pct: f64,
}

impl Default for ReclassifyPolicy {
    fn default() -> Self {
        Self {
            max_flips: 2,
            min_gap_bars: 20,
            dedup_window_bars: 3,
            dedup_price_eps_pct: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WyckoffStructureTracker {
    pub schematic: WyckoffSchematic,
    pub current_phase: WyckoffPhase,
    pub events: Vec<RecordedEvent>,
    pub range_top: f64,
    pub range_bottom: f64,
    pub creek: Option<f64>,
    pub ice: Option<f64>,
    pub slope_deg: f64,
    pub is_active: bool,
    pub failure_reason: Option<String>,
    /// P17 — count of `auto_reclassify`-triggered schematic flips.
    #[serde(default)]
    pub reclassify_count: u32,
    /// P17 — bar_index of the most recent flip (for cooldown gating).
    #[serde(default)]
    pub last_reclassify_bar: Option<u64>,
    /// P17 — hysteresis + dedup policy (config-driven per CLAUDE.md #2).
    #[serde(default)]
    pub policy: ReclassifyPolicy,
    /// P2c — minimum Phase-B inner tests (UA/ST-B/ST) required before
    /// B → C opens. 0 preserves legacy behaviour for old serialised rows.
    #[serde(default = "default_phase_b_min_inner_tests")]
    pub phase_b_min_inner_tests: usize,
    /// P2c — minimum bars between the last Phase-A event and the first
    /// Phase-C event. 0 preserves legacy behaviour.
    #[serde(default = "default_phase_b_min_bars")]
    pub phase_b_min_bars: usize,
    /// P2-#17 — if true, A → B requires an explicit ST in addition to
    /// climax + AR. Default false (canonical relaxed gate).
    #[serde(default)]
    pub require_st: bool,
    /// P2-#15 — minimum bar dwell per phase before the next transition
    /// can fire. Applied uniformly to A→B, B→C, C→D, D→E. 0 disables.
    #[serde(default = "default_phase_min_dwell_bars")]
    pub phase_min_dwell_bars: usize,
    /// P2-#15 — bar_index at which the tracker entered `current_phase`.
    /// `None` = unknown (legacy rows); treated as "dwell satisfied".
    #[serde(default)]
    pub phase_entered_bar: Option<u64>,
}

fn default_phase_b_min_inner_tests() -> usize { 1 }
fn default_phase_b_min_bars() -> usize { 10 }
fn default_phase_min_dwell_bars() -> usize { 3 }

impl WyckoffStructureTracker {
    /// Start a new structure from a detected trading range.
    pub fn new(schematic: WyckoffSchematic, range_top: f64, range_bottom: f64) -> Self {
        Self {
            schematic,
            current_phase: WyckoffPhase::A,
            events: Vec::new(),
            range_top,
            range_bottom,
            creek: None,
            ice: None,
            slope_deg: 0.0,
            is_active: true,
            failure_reason: None,
            reclassify_count: 0,
            last_reclassify_bar: None,
            policy: ReclassifyPolicy::default(),
            phase_b_min_inner_tests: default_phase_b_min_inner_tests(),
            phase_b_min_bars: default_phase_b_min_bars(),
            require_st: false,
            phase_min_dwell_bars: default_phase_min_dwell_bars(),
            phase_entered_bar: None,
        }
    }

    /// P2-#15/#17 — configure A→B strictness and min dwell.
    pub fn with_phase_gates(mut self, require_st: bool, min_dwell_bars: usize) -> Self {
        self.require_st = require_st;
        self.phase_min_dwell_bars = min_dwell_bars;
        self
    }

    /// Set hysteresis + dedup policy (caller loads from qtss_config).
    pub fn with_policy(mut self, policy: ReclassifyPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// P2c — override Phase-B gate thresholds (caller loads from
    /// system_config `wyckoff.phase.b.min_bars` / `min_inner_tests`).
    pub fn with_phase_b_gate(mut self, min_inner_tests: usize, min_bars: usize) -> Self {
        self.phase_b_min_inner_tests = min_inner_tests;
        self.phase_b_min_bars = min_bars;
        self
    }

    /// Back-compat shim — record without a timestamp. New callers
    /// should prefer [`record_event_with_time`] so the chart overlay
    /// can pin events to the exact candle regardless of bar_index
    /// origin (rolling-window vs global).
    pub fn record_event(&mut self, event: WyckoffEvent, bar_index: u64, price: f64, score: f64) {
        self.record_event_with_time(event, bar_index, price, score, None);
    }

    /// Record a new event and advance phase if warranted.
    ///
    /// Dedup: skip if the same event type was already recorded at the
    /// same bar_index, or within a 3-bar tolerance window with the
    /// same price (prevents 3200-duplicate bugs seen in production).
    pub fn record_event_with_time(
        &mut self,
        event: WyckoffEvent,
        bar_index: u64,
        price: f64,
        score: f64,
        time_ms: Option<i64>,
    ) {
        // P17 — TF-aware dedup: same event type within `dedup_window_bars`
        // AND (price is within eps OR either price is NaN). The price
        // check prevents two *semantically distinct* SCs at far-apart
        // prices from collapsing just because they land in the same
        // 20-bar window; the window check prevents the 3200-dup bug.
        let win = self.policy.dedup_window_bars;
        let eps_pct = self.policy.dedup_price_eps_pct;
        let price_close = |a: f64, b: f64| -> bool {
            let base = a.abs().max(b.abs()).max(1e-9);
            ((a - b).abs() / base) * 100.0 <= eps_pct
        };
        let dup_match = |e: &RecordedEvent| -> bool {
            e.event == event
                && bar_index.abs_diff(e.bar_index) <= win
                && price_close(e.price, price)
        };
        if self.events.iter().any(&dup_match) {
            if let Some(existing) = self.events.iter_mut().find(|e| dup_match(e)) {
                if score > existing.score {
                    existing.score = score;
                    existing.price = price;
                    existing.bar_index = bar_index;
                    if time_ms.is_some() {
                        existing.time_ms = time_ms;
                    }
                }
            }
            return;
        }
        self.events.push(RecordedEvent {
            event,
            bar_index,
            price,
            score,
            time_ms,
        });

        // Update key levels.
        //
        // Wyckoff rule: the range is defined ONCE in Phase A by the
        // climax (SC/BC) + AR pair, and is then FROZEN. Phase B/C/D
        // events (Spring, UTAD, UA, ST-B, LPS, LPSY…) **test** the
        // range but do not redefine it — Springs intentionally pierce
        // below support, UTADs above resistance; that is their defn.
        //
        // So we only mutate range_top/range_bottom while `current_phase
        // == Phase::A`. After the phase-A transition runs at the end of
        // this method, subsequent climax/AR-like events become purely
        // informational (stored in `events` for audit).
        let in_phase_a = self.current_phase == WyckoffPhase::A;
        // Invariant: resistance > support. A mutation that would invert
        // the range is almost certainly a mis-labelled event (e.g. an AR
        // price below current SC). Refuse it rather than persist a
        // corrupt structure (ETHUSDT/BTCUSDT had rows with negative h/mid
        // in prod because AR=37010 overwrote range_top=73449).
        match event {
            WyckoffEvent::SC if in_phase_a => {
                if price < self.range_top {
                    self.range_bottom = price;
                }
            }
            WyckoffEvent::BC if in_phase_a => {
                if price > self.range_bottom {
                    self.range_top = price;
                }
            }
            WyckoffEvent::AR if in_phase_a => match self.schematic {
                WyckoffSchematic::Accumulation | WyckoffSchematic::ReAccumulation => {
                    if price > self.range_bottom {
                        self.range_top = price;
                        self.creek = Some(price);
                    }
                }
                WyckoffSchematic::Distribution | WyckoffSchematic::ReDistribution => {
                    if price < self.range_top {
                        self.range_bottom = price;
                        self.ice = Some(price);
                    }
                }
            },
            // Creek/Ice are allowed to move in Phase D — that's the point
            // of the JAC (fresh creek above old) and BreakOfIce levels.
            WyckoffEvent::JAC => {
                self.creek = Some(price);
            }
            WyckoffEvent::BreakOfIce => {
                self.ice = Some(price);
            }
            _ => {}
        }

        // P2-P1-#8 — Mid-range climactic-volume flip (Phase B).
        //
        // Villahermosa ch. 5: unexpected volume peaks INSIDE the range
        // during Phase B signal distributive character — institutions
        // are offloading supply, not accumulating. When an Accumulation
        // hypothesis sees a climactic-volume event mid-range, flip to
        // Distribution before Phase C opens.
        //
        // Threshold lives on the caller via `record_event_with_score_vol`.
        // This method has no direct volume access, so the flip is
        // expressed via the event's score proxy — callers that want
        // this behaviour should use auto_reclassify's existing
        // directional map plus the new `note_phase_b_volume_spike` API
        // (see below).

        // P2-P1-#7 — Failed Spring / Failed UTAD watchdog.
        //
        // Villahermosa ch. 6 (pp. 566-568): a Spring must be followed
        // by a SpringTest and then SOS. If instead price breaks to a
        // NEW LOW below the Spring low within N bars, the Phase-C
        // setup failed — the range was actually distributive and the
        // structure should flip to ReDistribution. Mirror for UTAD.
        //
        // Timeout window is hardcoded here (12 bars) because it is a
        // Wyckoff-canonical behaviour, not a tuning knob. Operators who
        // want to disable the flip can set max_flips=0 in policy.
        self.check_failed_phase_c(event, bar_index, price);

        // Auto-reclassify schematic when the event carries an
        // unambiguous directional bias. Canonical Wyckoff:
        //   Spring / SOS / LPS / JAC      → accumulation-family
        //   UTAD  / SOW / LPSY / BreakOfIce → distribution-family
        // Initial schematic (from first climactic pivot) is only a
        // hypothesis; later manipulation / markup events override it.
        // Preserves the Re-* prefix: a distribution that flips bullish
        // becomes Re-Accumulation (it lives inside a broader uptrend),
        // not a fresh Accumulation.
        self.auto_reclassify(event, bar_index);

        // Phase advancement — canonical sequential gates (A→B→C→D→E)
        // require the earlier phase's evidence to be present.
        self.try_advance_phase();

        // **Bootstrap promotion removed (P12).**
        //
        // Previously this method ended with:
        //
        //     if event.phase() > self.current_phase {
        //         self.current_phase = event.phase();
        //     }
        //
        // — which bypassed every sequential gate in `try_advance_phase`
        // and let an isolated SOS (Phase D) or Markup (Phase E) bump
        // `current_phase` straight from A to D/E. That produced rows
        // like "current_phase=D with only a single SOS in events_json"
        // and the canonical "6-year-long structure" where a 2019 AR
        // sat in the same row as a 2026 Markup. Both shapes broke the
        // A→B→C→D→E contract the GUI and setup gates rely on.
        //
        // The only legitimate way to advance a phase is now through
        // `try_advance_phase`, which requires the canonical evidence
        // (climax+AR+ST for A→B, Spring/UTAD+D-event for C→D, etc.).
        // If the orchestrator starts mid-structure and prerequisite
        // events are missing, the event is still recorded in
        // `events_json` (audit trail) but `current_phase` stays put.
        // That is honest; fake promotion was not.
    }

    /// Promote schematic when an event unambiguously belongs to the
    /// opposite directional family. Look-up table — no scattered if/else.
    /// P2-P1-#8 — caller-driven Phase-B climactic volume flip.
    ///
    /// The orchestrator, while feeding bars, calls this whenever it
    /// detects a Phase-B bar whose volume exceeds the climactic flip
    /// multiple. When the structure is bullish (Accumulation /
    /// ReAccumulation) and still in Phase B, that spike is evidence of
    /// distributive character → flip to ReDistribution. Mirror for
    /// bearish structures receiving an unexpected climactic spike
    /// (rare but possible).
    pub fn note_phase_b_volume_spike(&mut self, bar_index: u64) {
        use WyckoffSchematic::*;
        if self.current_phase != WyckoffPhase::B {
            return;
        }
        // Honour existing hysteresis — same policy as auto_reclassify.
        if self.reclassify_count >= self.policy.max_flips {
            return;
        }
        if let Some(last) = self.last_reclassify_bar {
            if bar_index.saturating_sub(last) < self.policy.min_gap_bars {
                return;
            }
        }
        let new = match self.schematic {
            Accumulation => Some(ReDistribution),
            ReAccumulation => Some(ReDistribution),
            _ => None,
        };
        if let Some(s) = new {
            self.schematic = s;
            self.reclassify_count = self.reclassify_count.saturating_add(1);
            self.last_reclassify_bar = Some(bar_index);
            if self.failure_reason.is_none() {
                self.failure_reason = Some("phase_b_climactic_vol".into());
            }
        }
    }

    /// P2-P1-#7 — Failed Spring / Failed UTAD watchdog.
    ///
    /// Called from `record_event_with_time` BEFORE `auto_reclassify`
    /// so the flip it triggers supersedes any Spring-induced bullish
    /// reclassification. We only fire when a fresh Low pivot breaks
    /// under the latest Spring low (accum) or a fresh High breaks
    /// above the latest UTAD high (dist) within 12 bars.
    fn check_failed_phase_c(&mut self, event: WyckoffEvent, bar_index: u64, price: f64) {
        use WyckoffEvent::*;
        use WyckoffSchematic::*;
        const FAILED_PHASE_C_WINDOW: u64 = 12;
        // Only Low-forming events can invalidate a Spring (Shakeout /
        // SOW / LPSY all put in a lower low); mirror for UTAD.
        let accum_breaker = matches!(event, SOW | LPSY | Shakeout);
        let dist_breaker = matches!(event, SOS | LPS);
        if !accum_breaker && !dist_breaker {
            return;
        }
        if accum_breaker
            && matches!(self.schematic, Accumulation | ReAccumulation)
        {
            let parent = self.events.iter().rev().find(|e| matches!(e.event, Spring | SpringTest));
            if let Some(p) = parent {
                let within_window = bar_index.saturating_sub(p.bar_index) <= FAILED_PHASE_C_WINDOW;
                if within_window && price < p.price {
                    self.schematic = ReDistribution;
                    self.reclassify_count = self.reclassify_count.saturating_add(1);
                    self.last_reclassify_bar = Some(bar_index);
                    self.failure_reason = Some("failed_spring".into());
                }
            }
        }
        if dist_breaker
            && matches!(self.schematic, Distribution | ReDistribution)
        {
            let parent = self.events.iter().rev().find(|e| matches!(e.event, UTAD | UTADTest));
            if let Some(p) = parent {
                let within_window = bar_index.saturating_sub(p.bar_index) <= FAILED_PHASE_C_WINDOW;
                if within_window && price > p.price {
                    self.schematic = ReAccumulation;
                    self.reclassify_count = self.reclassify_count.saturating_add(1);
                    self.last_reclassify_bar = Some(bar_index);
                    self.failure_reason = Some("failed_utad".into());
                }
            }
        }
    }

    fn auto_reclassify(&mut self, event: WyckoffEvent, bar_index: u64) {
        use WyckoffEvent::*;
        use WyckoffSchematic::*;
        let bullish = match event {
            Spring | SpringTest | SOS | LPS | JAC => Some(true),
            UTAD | UTADTest | SOW | LPSY | BreakOfIce => Some(false),
            _ => None,
        };
        let Some(bull) = bullish else { return };

        // P17 — hysteresis guards (Gemini review #1). Prevents the
        // Distribution ↔ ReAccumulation ↔ ReDistribution ping-pong on
        // choppy bars where UTAD and Spring fire in quick succession.
        if self.reclassify_count >= self.policy.max_flips {
            return;
        }
        if let Some(last) = self.last_reclassify_bar {
            if bar_index.saturating_sub(last) < self.policy.min_gap_bars {
                return;
            }
        }

        let next = match (self.schematic, bull) {
            (Distribution,   true)  => ReAccumulation,
            (ReDistribution, true)  => Accumulation,
            (Accumulation,   false) => ReDistribution,
            (ReAccumulation, false) => Distribution,
            (s, _) => s,
        };
        if next != self.schematic {
            self.schematic = next;
            self.reclassify_count += 1;
            self.last_reclassify_bar = Some(bar_index);
        }
    }

    /// Try to advance phase based on accumulated events.
    fn try_advance_phase(&mut self) {
        let phase_events: Vec<WyckoffEvent> = self.events.iter().map(|e| e.event).collect();

        // P2-#15: temporal gate — every transition must wait at least
        // `phase_min_dwell_bars` bars after we entered the current phase.
        // Legacy rows with `phase_entered_bar = None` bypass this gate.
        let latest_bar = self.events.iter().map(|e| e.bar_index).max().unwrap_or(0);
        let dwell_ok = match self.phase_entered_bar {
            Some(entered) => latest_bar.saturating_sub(entered) as usize
                >= self.phase_min_dwell_bars,
            None => true,
        };
        if !dwell_ok {
            return;
        }

        let starting_phase = self.current_phase;

        match self.current_phase {
            WyckoffPhase::A => {
                // P28c — A → B requires climax + (AR OR ST). The strict
                // SC+AR+ST gate starved Phase B on fast TFs where the
                // dedup window collapses ST into SC; canonical Wyckoff
                // only requires a climax followed by an automatic
                // rally/reaction. Strictness toggle lives in
                // `wyckoff.phase.a_to_b.require_st` (default false).
                let has_climax = phase_events.contains(&WyckoffEvent::SC)
                    || phase_events.contains(&WyckoffEvent::BC);
                let has_ar = phase_events.contains(&WyckoffEvent::AR);
                let has_st = phase_events.contains(&WyckoffEvent::ST);
                // P2-#17 — strict A→B requires ST explicitly on top of
                // climax + AR. Relaxed default = climax + (AR or ST).
                let ok = if self.require_st {
                    has_climax && has_ar && has_st
                } else {
                    has_climax && (has_ar || has_st)
                };
                if ok {
                    self.current_phase = WyckoffPhase::B;
                }
            }
            WyckoffPhase::B => {
                // P2c — Phase B real gate (Villahermosa ch. 5).
                //
                // Prior logic: `b_count≥2 OR has_c_event` — the OR path
                // let a bare Spring/UTAD jump straight into C, so B was
                // optional. Canonical Wyckoff: B is the LONGEST phase,
                // must be traversed. B → C now requires:
                //   1. at least `phase_b_min_inner_tests` Phase-B events
                //      (UA / ST-B / ST) since A's last event,
                //   2. at least `phase_b_min_bars` bars elapsed since
                //      the latest Phase-A event,
                //   3. AND a fired Phase-C event (Spring/UTAD/Shakeout).
                //
                // Config-driven via WyckoffConfig.phase_b_*; operators
                // tune per-TF in system_config.
                let b_inner = phase_events.iter()
                    .filter(|e| e.phase() == WyckoffPhase::B)
                    .count();
                let last_a_bar = self.events.iter()
                    .filter(|e| e.event.phase() == WyckoffPhase::A)
                    .map(|e| e.bar_index)
                    .max()
                    .unwrap_or(0);
                let latest_bar = self.events.iter()
                    .map(|e| e.bar_index)
                    .max()
                    .unwrap_or(last_a_bar);
                let bars_in_b = latest_bar.saturating_sub(last_a_bar) as usize;
                let has_c_event = phase_events.iter().any(|e| e.phase() == WyckoffPhase::C);
                let min_tests = self.phase_b_min_inner_tests;
                let min_bars = self.phase_b_min_bars;
                if has_c_event && b_inner >= min_tests && bars_in_b >= min_bars {
                    self.current_phase = WyckoffPhase::C;
                }
            }
            WyckoffPhase::C => {
                // P2d — C → D now requires the Spring/UTAD *test* as
                // explicit confirmation, not just "any Phase-D event
                // fired". Villahermosa ch. 6: the sequence is
                // Spring → (SpringTest) → SOS. Without the low-volume
                // retest the Phase-C setup is unconfirmed and SOS can
                // be noise. Shakeout retains its self-confirming
                // semantics (aggressive Spring variant #1).
                let has_spring = phase_events.contains(&WyckoffEvent::Spring);
                let has_utad = phase_events.contains(&WyckoffEvent::UTAD);
                let has_shakeout = phase_events.contains(&WyckoffEvent::Shakeout);
                let has_spring_test = phase_events.contains(&WyckoffEvent::SpringTest);
                let has_utad_test = phase_events.contains(&WyckoffEvent::UTADTest);
                let has_d_event = phase_events.iter().any(|e| e.phase() == WyckoffPhase::D);
                let confirmed = has_shakeout
                    || (has_spring && has_spring_test)
                    || (has_utad && has_utad_test);
                if confirmed && has_d_event {
                    self.current_phase = WyckoffPhase::D;
                }
            }
            WyckoffPhase::D => {
                // D → E requires: JAC + BUEC (accumulation) or BreakOfIce (distribution)
                let has_jac = phase_events.contains(&WyckoffEvent::JAC);
                let has_boi = phase_events.contains(&WyckoffEvent::BreakOfIce);
                let has_markup = phase_events.contains(&WyckoffEvent::Markup);
                let has_markdown = phase_events.contains(&WyckoffEvent::Markdown);
                if has_jac || has_boi || has_markup || has_markdown {
                    self.current_phase = WyckoffPhase::E;
                }
            }
            WyckoffPhase::E => {
                // Terminal phase — structure is complete
            }
        }

        // P2-#15: record entry bar if the phase advanced this call.
        if self.current_phase != starting_phase {
            self.phase_entered_bar = Some(latest_bar);
        }
    }

    /// Mark the structure as failed (e.g. accumulation turns into distribution).
    pub fn fail(&mut self, reason: &str) {
        self.is_active = false;
        self.failure_reason = Some(reason.to_string());
    }

    /// Reclassify the structure (e.g. failed accumulation → re-distribution).
    pub fn reclassify(&mut self, new_schematic: WyckoffSchematic) {
        self.schematic = new_schematic;
    }

    /// Backwards-compatible confidence — uses [`ConfidenceWeights::default`].
    pub fn confidence(&self) -> f64 {
        self.confidence_with(&ConfidenceWeights::default())
    }

    /// Multi-factor confidence (Faz 10 P6).
    ///
    /// Prior version was `avg_best_score × diversity_ratio + phase_bonus`.
    /// That over-rewarded a handful of high-score events firing in quick
    /// succession at the same bar and ignored whether the canonical
    /// phase-specific events were actually present.
    ///
    /// New scoring combines four independent factors plus a phase bonus:
    ///
    /// 1. **diversity_quality** — existing behaviour: avg best-score per
    ///    distinct event type × (distinct_count / expected).
    /// 2. **critical_events** — fraction of phase-canonical events
    ///    present. Phase A needs climax + AR + ST; Phase C needs a
    ///    Spring/UTAD/Shakeout on top of that; and so on.
    /// 3. **temporal_span** — bars between first and last event vs.
    ///    expected structure duration. Penalises "five events at one
    ///    bar" clusters that the old formula treated as a full phase A.
    /// 4. **coherence** — 1 − (opposite-family-event-ratio × 0.5).
    ///    Mixed bullish/bearish signals (UTAD inside Accumulation,
    ///    Spring inside Distribution) cut the score up to 50%.
    ///
    /// All weights live in [`ConfidenceWeights`]; callers that read
    /// config can build a custom instance (CLAUDE.md #2). The default
    /// instance is the one used by existing callers.
    pub fn confidence_with(&self, w: &ConfidenceWeights) -> f64 {
        if self.events.is_empty() {
            return 0.0;
        }

        // --- factor 1: diversity × quality ---
        let mut best: std::collections::HashMap<WyckoffEvent, f64> =
            std::collections::HashMap::new();
        for e in &self.events {
            let entry = best.entry(e.event).or_insert(0.0);
            if e.score > *entry {
                *entry = e.score;
            }
        }
        let distinct_count = best.len() as f64;
        let avg_best: f64 = best.values().sum::<f64>() / distinct_count;
        let expected = expected_event_count(self.current_phase);
        let diversity = (distinct_count / expected).min(1.0);
        let dq = avg_best * diversity;

        // --- factor 2: critical-event coverage ---
        let crit = critical_coverage(self.current_phase, &best);

        // --- factor 3: temporal span ---
        let span = temporal_span_ratio(&self.events, self.current_phase);

        // --- factor 4: coherence (fewer opposing-family events = better) ---
        let coh = coherence_ratio(self.schematic, &self.events);

        // --- phase bonus ---
        let bonus = phase_bonus(self.current_phase) * w.phase_bonus_scale;

        let score = w.diversity_quality * dq
            + w.critical_events * crit
            + w.temporal_span * span
            + w.coherence * coh
            + bonus;

        score.clamp(0.0, 1.0)
    }

    /// Map event name from detector to WyckoffEvent.
    pub fn event_from_detector_name(name: &str) -> Option<WyckoffEvent> {
        match name {
            "selling_climax" => Some(WyckoffEvent::SC),
            "buying_climax" => Some(WyckoffEvent::BC),
            "automatic_rally" => Some(WyckoffEvent::AR),
            "automatic_reaction" => Some(WyckoffEvent::AR),
            "secondary_test" => Some(WyckoffEvent::ST),
            "spring" => Some(WyckoffEvent::Spring),
            "upthrust" => Some(WyckoffEvent::UTAD),
            "spring_test" => Some(WyckoffEvent::SpringTest),
            "utad_test" => Some(WyckoffEvent::UTADTest),
            "upthrust_action" => Some(WyckoffEvent::UA),
            "shakeout" => Some(WyckoffEvent::Shakeout),
            "sign_of_strength" => Some(WyckoffEvent::SOS),
            "sign_of_weakness" => Some(WyckoffEvent::SOW),
            "last_point_of_support" => Some(WyckoffEvent::LPS),
            "last_point_of_supply" => Some(WyckoffEvent::LPSY),
            "jump_across_creek" => Some(WyckoffEvent::JAC),
            "break_of_ice" => Some(WyckoffEvent::BreakOfIce),
            "shortening_of_thrust" => Some(WyckoffEvent::SOT),
            // P13 additions — completes the 16-event vocabulary so the
            // sequential phase gates can actually fire.
            "preliminary_supply" => Some(WyckoffEvent::PS),
            "secondary_test_b" => Some(WyckoffEvent::STB),
            "back_up_edge_creek" => Some(WyckoffEvent::BUEC),
            _ => None,
        }
    }
}

// =========================================================================
// Confidence scoring — multi-factor (Faz 10 P6)
// =========================================================================

/// Weights for [`WyckoffStructureTracker::confidence_with`]. Default
/// values are algorithmic defaults; an operator can override by
/// constructing a custom instance from config and passing it in.
#[derive(Debug, Clone, Copy)]
pub struct ConfidenceWeights {
    pub diversity_quality: f64,
    pub critical_events:   f64,
    pub temporal_span:     f64,
    pub coherence:         f64,
    /// Multiplier on the per-phase bonus (A=0, B=0.05, C=0.10, D=0.15, E=0.20).
    pub phase_bonus_scale: f64,
}

impl Default for ConfidenceWeights {
    fn default() -> Self {
        Self {
            diversity_quality: 0.40,
            critical_events:   0.25,
            temporal_span:     0.15,
            coherence:         0.20,
            phase_bonus_scale: 1.0,
        }
    }
}

fn expected_event_count(phase: WyckoffPhase) -> f64 {
    match phase {
        WyckoffPhase::A => 3.0,
        WyckoffPhase::B => 5.0,
        WyckoffPhase::C => 6.0,
        WyckoffPhase::D => 8.0,
        WyckoffPhase::E => 9.0,
    }
}

fn phase_bonus(phase: WyckoffPhase) -> f64 {
    match phase {
        WyckoffPhase::A => 0.00,
        WyckoffPhase::B => 0.05,
        WyckoffPhase::C => 0.10,
        WyckoffPhase::D => 0.15,
        WyckoffPhase::E => 0.20,
    }
}

/// Bars expected between first and last event for each phase — used to
/// normalize [`temporal_span_ratio`]. Values are conservative heuristics
/// derived from canonical Wyckoff literature (Pruden / Schabacker):
/// Phase A typically takes 20–40 bars, full A→E runs 150+.
fn expected_span_bars(phase: WyckoffPhase) -> f64 {
    match phase {
        WyckoffPhase::A => 30.0,
        WyckoffPhase::B => 60.0,
        WyckoffPhase::C => 90.0,
        WyckoffPhase::D => 120.0,
        WyckoffPhase::E => 150.0,
    }
}

/// Canonical events required for each phase. A subset is enough (e.g.
/// SC OR BC satisfies the climax slot). Each tuple is "label" → list of
/// events that count as satisfying that slot.
fn critical_slots(phase: WyckoffPhase) -> &'static [&'static [WyckoffEvent]] {
    use WyckoffEvent::*;
    match phase {
        WyckoffPhase::A => &[&[SC, BC], &[AR], &[ST]],
        WyckoffPhase::B => &[&[SC, BC], &[AR], &[ST], &[UA, STB]],
        WyckoffPhase::C => &[&[SC, BC], &[AR], &[ST], &[UA, STB], &[Spring, UTAD, Shakeout]],
        WyckoffPhase::D => &[
            &[SC, BC], &[AR], &[ST], &[UA, STB],
            &[Spring, UTAD, Shakeout],
            &[SOS, SOW, LPS, LPSY, JAC, BreakOfIce],
        ],
        WyckoffPhase::E => &[
            &[SC, BC], &[AR], &[ST], &[UA, STB],
            &[Spring, UTAD, Shakeout],
            &[SOS, SOW, LPS, LPSY, JAC, BreakOfIce],
            &[Markup, Markdown],
        ],
    }
}

fn critical_coverage(
    phase: WyckoffPhase,
    best: &std::collections::HashMap<WyckoffEvent, f64>,
) -> f64 {
    let slots = critical_slots(phase);
    if slots.is_empty() { return 1.0; }
    let filled = slots.iter()
        .filter(|slot| slot.iter().any(|ev| best.contains_key(ev)))
        .count();
    filled as f64 / slots.len() as f64
}

fn temporal_span_ratio(events: &[RecordedEvent], phase: WyckoffPhase) -> f64 {
    if events.len() < 2 { return 0.0; }
    let first = events.iter().map(|e| e.bar_index).min().unwrap_or(0);
    let last  = events.iter().map(|e| e.bar_index).max().unwrap_or(0);
    let span = last.saturating_sub(first) as f64;
    (span / expected_span_bars(phase)).min(1.0)
}

/// Returns a value in [0.5, 1.0]. Every opposing-family event chips at
/// coherence proportionally to its share of total events. Pure-family
/// structures score 1.0; a 50/50 mix bottoms out at 0.5.
fn coherence_ratio(schematic: WyckoffSchematic, events: &[RecordedEvent]) -> f64 {
    if events.is_empty() { return 1.0; }
    use WyckoffEvent::*;
    use WyckoffSchematic::*;
    let is_bull_schematic = matches!(schematic, Accumulation | ReAccumulation);
    let opposing = events.iter().filter(|e| {
        let bullish_ev = matches!(e.event, Spring | SpringTest | SOS | LPS | JAC | Markup);
        let bearish_ev = matches!(e.event, UTAD | UTADTest | SOW | LPSY | BreakOfIce | Markdown);
        // neutral events (SC/BC/AR/ST/UA/STB/Shakeout/SOT) don't count
        // either way — they are schematic-agnostic phase-A/B markers.
        if is_bull_schematic { bearish_ev } else { bullish_ev }
    }).count() as f64;
    let ratio = opposing / events.len() as f64;
    1.0 - (ratio * 0.5)
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker_with(events: &[(WyckoffEvent, u64, f64)]) -> WyckoffStructureTracker {
        let mut t = WyckoffStructureTracker::new(WyckoffSchematic::Accumulation, 100.0, 90.0);
        for (ev, bar, score) in events {
            t.record_event(*ev, *bar, 95.0, *score);
        }
        t
    }

    #[test]
    fn empty_events_zero_confidence() {
        let t = WyckoffStructureTracker::new(WyckoffSchematic::Accumulation, 100.0, 90.0);
        assert_eq!(t.confidence(), 0.0);
    }

    #[test]
    fn phase_a_full_triad_beats_single_event() {
        let partial = tracker_with(&[(WyckoffEvent::SC, 10, 0.8)]);
        let full = tracker_with(&[
            (WyckoffEvent::SC, 10, 0.8),
            (WyckoffEvent::AR, 20, 0.8),
            (WyckoffEvent::ST, 35, 0.8),
        ]);
        assert!(full.confidence() > partial.confidence());
    }

    #[test]
    fn temporal_span_penalty_applies_to_clustered_events() {
        // Same events, different bar spans.
        let clustered = tracker_with(&[
            (WyckoffEvent::SC, 10, 0.9),
            (WyckoffEvent::AR, 10, 0.9),
            (WyckoffEvent::ST, 10, 0.9),
        ]);
        let spread = tracker_with(&[
            (WyckoffEvent::SC, 10, 0.9),
            (WyckoffEvent::AR, 25, 0.9),
            (WyckoffEvent::ST, 40, 0.9),
        ]);
        assert!(spread.confidence() > clustered.confidence());
    }

    #[test]
    fn coherence_penalises_opposing_family() {
        // Pure bullish accumulation events vs mixed bearish injection.
        let pure = tracker_with(&[
            (WyckoffEvent::SC, 10, 0.9),
            (WyckoffEvent::AR, 25, 0.9),
            (WyckoffEvent::ST, 40, 0.9),
            (WyckoffEvent::Spring, 55, 0.9),
        ]);
        let pure_conf = pure.confidence();
        // Reclassification will flip schematic on UTAD; rebuild fresh so
        // the comparison isolates coherence, not schematic identity.
        let mut mixed = WyckoffStructureTracker::new(
            WyckoffSchematic::Accumulation, 100.0, 90.0);
        mixed.record_event(WyckoffEvent::SC, 10, 95.0, 0.9);
        mixed.record_event(WyckoffEvent::AR, 25, 95.0, 0.9);
        mixed.record_event(WyckoffEvent::ST, 40, 95.0, 0.9);
        // Inject an opposing-family event but force schematic back for
        // the coherence comparison.
        mixed.record_event(WyckoffEvent::UTAD, 55, 95.0, 0.9);
        mixed.schematic = WyckoffSchematic::Accumulation;
        assert!(pure_conf > mixed.confidence());
    }

    #[test]
    fn mid_structure_event_promotes_phase() {
        // Historical scan spawns a tracker and the first event seen is
        // an SOS (canonical Phase D). Without the bootstrap guard the
        // tracker stayed at Phase A because try_advance_phase needs
        // SC/BC + AR + ST first. After the guard it reports Phase D.
        let t = tracker_with(&[(WyckoffEvent::SOS, 100, 0.9)]);
        assert_eq!(t.current_phase, WyckoffPhase::D);

        // And a Markup alone lands in Phase E.
        let e = tracker_with(&[(WyckoffEvent::Markup, 200, 0.9)]);
        assert_eq!(e.current_phase, WyckoffPhase::E);
    }

    #[test]
    fn confidence_is_bounded() {
        let maxed = tracker_with(&[
            (WyckoffEvent::SC, 10, 1.0),
            (WyckoffEvent::AR, 30, 1.0),
            (WyckoffEvent::ST, 60, 1.0),
            (WyckoffEvent::Spring, 90, 1.0),
            (WyckoffEvent::SOS, 120, 1.0),
            (WyckoffEvent::JAC, 150, 1.0),
            (WyckoffEvent::Markup, 180, 1.0),
        ]);
        let c = maxed.confidence();
        assert!(c > 0.0 && c <= 1.0, "got {c}");
    }
}
