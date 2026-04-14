//! Wyckoff Setup Builder — translates a tracked WyckoffStructure + recent bars
//! into trade-able setup candidates with **classical Wyckoff levels** (L1).
//!
//! The L1 values (entry, sl, tp_targets, pnf_target) come from pattern geometry
//! and are immutable per setup type. The Q-RADAR adaptive layer (L2) lives in
//! `trade_planner.rs` and consumes these candidates to produce the final
//! `tp_ladder` and `entry_sl` written to `qtss_v2_setups`.
//!
//! CLAUDE.md compliance:
//! - #1: each setup is its own `impl SetupBuilder` (no central match).
//! - #2: every threshold (buffers, P&F factor, min phase) read from
//!   `WyckoffSetupConfig` (config table loader lives in `config.rs`).
//! - #3: this module is a **detector-side** helper — it produces candidates,
//!   not TradeIntents. Strategy/risk/execution layers handle the rest.

use serde::{Deserialize, Serialize};

use crate::structure::{
    RecordedEvent, WyckoffEvent, WyckoffPhase, WyckoffSchematic, WyckoffStructureTracker,
};

// =========================================================================
// Setup type & direction
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WyckoffSetupType {
    Spring,
    Lps,
    Buec,
    Ut,
    Utad,
    Lpsy,
    IceRetest,
}

impl WyckoffSetupType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spring => "wyckoff_spring",
            Self::Lps => "wyckoff_lps",
            Self::Buec => "wyckoff_buec",
            Self::Ut => "wyckoff_ut",
            Self::Utad => "wyckoff_utad",
            Self::Lpsy => "wyckoff_lpsy",
            Self::IceRetest => "wyckoff_ice_retest",
        }
    }

    pub fn direction(self) -> SetupDirection {
        match self {
            Self::Spring | Self::Lps | Self::Buec => SetupDirection::Long,
            Self::Ut | Self::Utad | Self::Lpsy | Self::IceRetest => SetupDirection::Short,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupDirection {
    Long,
    Short,
}

impl SetupDirection {
    pub fn sign(self) -> f64 {
        match self {
            Self::Long => 1.0,
            Self::Short => -1.0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Long => "long",
            Self::Short => "short",
        }
    }
}

// =========================================================================
// Light-weight bar (f64) — avoids Decimal overhead inside setup logic.
// Caller converts qtss-domain TimestampBar → SetupBar at the boundary.
// =========================================================================

#[derive(Debug, Clone, Copy)]
pub struct SetupBar {
    pub ts_ms: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

// =========================================================================
// Climax quality metrics (filled when trigger event is a climax bar)
// =========================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClimaxMetrics {
    pub volume_ratio: f64,   // bar volume / 20-bar avg
    pub spread_atr: f64,     // (high-low) / atr
    pub close_pos_pct: f64,  // (close - low) / (high - low)  ∈ [0,1]
    pub wick_pct: f64,       // total wick / range
}

// =========================================================================
// Configuration (passed in; loaded from system_config by caller)
// =========================================================================

#[derive(Debug, Clone)]
pub struct WyckoffSetupConfig {
    pub min_phase: WyckoffPhase,                // C or D
    pub min_range_bars: usize,                  // e.g. 20
    pub min_climax_volume_ratio: f64,           // e.g. 1.8
    pub allowed_types: Vec<WyckoffSetupType>,   // whitelist
    pub cause_effect_factor: f64,               // P&F target multiplier (1.0 = full range)
    pub spring_buffer_atr: f64,
    pub lps_buffer_atr: f64,
    pub ut_buffer_atr: f64,
    pub lpsy_buffer_atr: f64,
    pub buec_buffer_atr: f64,
    pub ice_retest_buffer_atr: f64,
}

impl Default for WyckoffSetupConfig {
    fn default() -> Self {
        Self {
            min_phase: WyckoffPhase::C,
            min_range_bars: 20,
            min_climax_volume_ratio: 1.8,
            allowed_types: vec![
                WyckoffSetupType::Spring,
                WyckoffSetupType::Lps,
                WyckoffSetupType::Buec,
                WyckoffSetupType::Ut,
                WyckoffSetupType::Utad,
                WyckoffSetupType::Lpsy,
                WyckoffSetupType::IceRetest,
            ],
            cause_effect_factor: 1.0,
            spring_buffer_atr: 0.5,
            lps_buffer_atr: 0.3,
            ut_buffer_atr: 0.5,
            lpsy_buffer_atr: 0.3,
            buec_buffer_atr: 0.4,
            ice_retest_buffer_atr: 0.4,
        }
    }
}

// =========================================================================
// Setup context — passed to every builder (CLAUDE.md #1: no central match)
// =========================================================================

pub struct SetupContext<'a> {
    pub tracker: &'a WyckoffStructureTracker,
    pub bars: &'a [SetupBar],   // most-recent-last
    pub atr: f64,                // ATR(14) at last bar
    pub vol_avg_20: f64,         // 20-bar volume average at last bar
    pub cfg: &'a WyckoffSetupConfig,
}

impl<'a> SetupContext<'a> {
    pub fn last(&self) -> Option<&SetupBar> { self.bars.last() }

    pub fn last_event(&self, ev: WyckoffEvent) -> Option<&RecordedEvent> {
        self.tracker.events.iter().rev().find(|e| e.event == ev)
    }

    pub fn range_height(&self) -> f64 {
        (self.tracker.range_top - self.tracker.range_bottom).abs()
    }

    pub fn climax_metrics(&self, bar: &SetupBar) -> ClimaxMetrics {
        let range = (bar.high - bar.low).max(1e-9);
        let vr = if self.vol_avg_20 > 0.0 { bar.volume / self.vol_avg_20 } else { 0.0 };
        let sa = if self.atr > 0.0 { range / self.atr } else { 0.0 };
        let cp = (bar.close - bar.low) / range;
        let upper_wick = bar.high - bar.open.max(bar.close);
        let lower_wick = bar.open.min(bar.close) - bar.low;
        let wick = (upper_wick + lower_wick) / range;
        ClimaxMetrics { volume_ratio: vr, spread_atr: sa, close_pos_pct: cp, wick_pct: wick }
    }
}

// =========================================================================
// Output: WyckoffSetupCandidate (L1 — classical Wyckoff levels)
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WyckoffSetupCandidate {
    pub setup_type: WyckoffSetupType,
    pub direction: SetupDirection,
    pub phase: WyckoffPhase,
    pub schematic: WyckoffSchematic,
    pub range_top: f64,
    pub range_bottom: f64,
    pub range_height: f64,
    pub pnf_target: f64,           // classical P&F projection
    pub entry: f64,
    pub sl: f64,
    pub tp_targets: Vec<f64>,      // classical Wyckoff TPs (creek/ice + projection)
    pub trigger_event: WyckoffEvent,
    pub trigger_bar_index: u64,
    pub trigger_bar_ts_ms: i64,
    pub trigger_price: f64,
    pub climax: ClimaxMetrics,
    pub atr_at_trigger: f64,
    pub raw_score: f64,            // pre-composite signal strength (0..100)
}

impl WyckoffSetupCandidate {
    /// Risk per unit (entry - sl in absolute, direction-aware).
    pub fn risk(&self) -> f64 { (self.entry - self.sl).abs() }

    /// R-multiple of a target price.
    pub fn r_of(&self, price: f64) -> f64 {
        let r = self.risk();
        if r <= 0.0 { return 0.0; }
        let dir = self.direction.sign();
        ((price - self.entry) * dir) / r
    }
}

// =========================================================================
// SetupBuilder trait — one impl per setup type (CLAUDE.md #1)
// =========================================================================

pub trait SetupBuilder: Send + Sync {
    fn setup_type(&self) -> WyckoffSetupType;
    fn try_build(&self, ctx: &SetupContext<'_>) -> Option<WyckoffSetupCandidate>;
}

/// Registry — every new setup adds one line here, no central match arm.
pub fn all_builders() -> Vec<Box<dyn SetupBuilder>> {
    vec![
        Box::new(SpringBuilder),
        Box::new(LpsBuilder),
        Box::new(BuecBuilder),
        Box::new(UtBuilder),
        Box::new(UtadBuilder),
        Box::new(LpsyBuilder),
        Box::new(IceRetestBuilder),
    ]
}

// =========================================================================
// Helpers (gate checks shared by builders — early return / CLAUDE.md #1)
// =========================================================================

fn gate_basic(ctx: &SetupContext<'_>, ty: WyckoffSetupType) -> Option<()> {
    if !ctx.cfg.allowed_types.contains(&ty) { return None; }
    if (ctx.tracker.current_phase as u8) < (ctx.cfg.min_phase as u8) { return None; }
    if ctx.tracker.events.len() < 2 { return None; }
    if ctx.bars.len() < ctx.cfg.min_range_bars { return None; }
    if ctx.atr <= 0.0 { return None; }
    Some(())
}

fn pnf_target(ctx: &SetupContext<'_>, dir: SetupDirection) -> f64 {
    let h = (ctx.tracker.range_top - ctx.tracker.range_bottom).abs();
    let proj = h * ctx.cfg.cause_effect_factor;
    match dir {
        SetupDirection::Long  => ctx.tracker.range_top + proj,
        SetupDirection::Short => ctx.tracker.range_bottom - proj,
    }
}

// =========================================================================
// 1. Spring — Phase C accumulation (false breakdown)
// =========================================================================
pub struct SpringBuilder;
impl SetupBuilder for SpringBuilder {
    fn setup_type(&self) -> WyckoffSetupType { WyckoffSetupType::Spring }

    fn try_build(&self, ctx: &SetupContext<'_>) -> Option<WyckoffSetupCandidate> {
        gate_basic(ctx, WyckoffSetupType::Spring)?;
        if !matches!(ctx.tracker.schematic,
            WyckoffSchematic::Accumulation | WyckoffSchematic::ReAccumulation) { return None; }
        let spring = ctx.last_event(WyckoffEvent::Spring)?.clone();
        let last = ctx.last()?;
        // Trigger: close back inside range above range_bottom
        if last.close < ctx.tracker.range_bottom { return None; }

        let sl = spring.price - ctx.cfg.spring_buffer_atr * ctx.atr;
        let entry = last.close.max(ctx.tracker.range_bottom + ctx.atr * 0.1);
        let target_proj = pnf_target(ctx, SetupDirection::Long);
        let tp_targets = vec![ctx.tracker.range_top, target_proj];

        Some(WyckoffSetupCandidate {
            setup_type: WyckoffSetupType::Spring,
            direction: SetupDirection::Long,
            phase: ctx.tracker.current_phase,
            schematic: ctx.tracker.schematic,
            range_top: ctx.tracker.range_top,
            range_bottom: ctx.tracker.range_bottom,
            range_height: ctx.range_height(),
            pnf_target: target_proj,
            entry,
            sl,
            tp_targets,
            trigger_event: WyckoffEvent::Spring,
            trigger_bar_index: spring.bar_index,
            trigger_bar_ts_ms: last.ts_ms,
            trigger_price: spring.price,
            climax: ctx.climax_metrics(last),
            atr_at_trigger: ctx.atr,
            raw_score: spring.score.clamp(0.0, 100.0),
        })
    }
}

// =========================================================================
// 2. LPS — Phase D accumulation (last-point-of-support pullback)
// =========================================================================
pub struct LpsBuilder;
impl SetupBuilder for LpsBuilder {
    fn setup_type(&self) -> WyckoffSetupType { WyckoffSetupType::Lps }

    fn try_build(&self, ctx: &SetupContext<'_>) -> Option<WyckoffSetupCandidate> {
        gate_basic(ctx, WyckoffSetupType::Lps)?;
        if !matches!(ctx.tracker.schematic,
            WyckoffSchematic::Accumulation | WyckoffSchematic::ReAccumulation) { return None; }
        if ctx.tracker.current_phase < WyckoffPhase::D { return None; }

        let lps = ctx.last_event(WyckoffEvent::LPS)?.clone();
        let last = ctx.last()?;
        // Trigger: bullish close above LPS pivot
        if last.close <= lps.price { return None; }

        let sl = lps.price - ctx.cfg.lps_buffer_atr * ctx.atr;
        let entry = last.close;
        let target_proj = pnf_target(ctx, SetupDirection::Long);
        let creek = ctx.tracker.creek.unwrap_or(ctx.tracker.range_top);
        let tp_targets = vec![creek, ctx.tracker.range_top, target_proj];

        Some(WyckoffSetupCandidate {
            setup_type: WyckoffSetupType::Lps,
            direction: SetupDirection::Long,
            phase: ctx.tracker.current_phase,
            schematic: ctx.tracker.schematic,
            range_top: ctx.tracker.range_top,
            range_bottom: ctx.tracker.range_bottom,
            range_height: ctx.range_height(),
            pnf_target: target_proj,
            entry,
            sl,
            tp_targets,
            trigger_event: WyckoffEvent::LPS,
            trigger_bar_index: lps.bar_index,
            trigger_bar_ts_ms: last.ts_ms,
            trigger_price: lps.price,
            climax: ctx.climax_metrics(last),
            atr_at_trigger: ctx.atr,
            raw_score: lps.score.clamp(0.0, 100.0),
        })
    }
}

// =========================================================================
// 3. BUEC — Back Up to Edge of Creek (creek breakout retest)
// =========================================================================
pub struct BuecBuilder;
impl SetupBuilder for BuecBuilder {
    fn setup_type(&self) -> WyckoffSetupType { WyckoffSetupType::Buec }

    fn try_build(&self, ctx: &SetupContext<'_>) -> Option<WyckoffSetupCandidate> {
        gate_basic(ctx, WyckoffSetupType::Buec)?;
        if !matches!(ctx.tracker.schematic,
            WyckoffSchematic::Accumulation | WyckoffSchematic::ReAccumulation) { return None; }
        let buec = ctx.last_event(WyckoffEvent::BUEC)?.clone();
        let last = ctx.last()?;
        let creek = ctx.tracker.creek.unwrap_or(ctx.tracker.range_top);
        // Trigger: price retested creek and bounced (close > creek)
        if last.close <= creek { return None; }

        let sl = creek - ctx.cfg.buec_buffer_atr * ctx.atr;
        let entry = last.close;
        let target_proj = pnf_target(ctx, SetupDirection::Long);
        let tp_targets = vec![ctx.tracker.range_top, target_proj];

        Some(WyckoffSetupCandidate {
            setup_type: WyckoffSetupType::Buec,
            direction: SetupDirection::Long,
            phase: ctx.tracker.current_phase,
            schematic: ctx.tracker.schematic,
            range_top: ctx.tracker.range_top,
            range_bottom: ctx.tracker.range_bottom,
            range_height: ctx.range_height(),
            pnf_target: target_proj,
            entry, sl, tp_targets,
            trigger_event: WyckoffEvent::BUEC,
            trigger_bar_index: buec.bar_index,
            trigger_bar_ts_ms: last.ts_ms,
            trigger_price: buec.price,
            climax: ctx.climax_metrics(last),
            atr_at_trigger: ctx.atr,
            raw_score: buec.score.clamp(0.0, 100.0),
        })
    }
}

// =========================================================================
// 4. UT — Phase C distribution (upthrust)
// =========================================================================
pub struct UtBuilder;
impl SetupBuilder for UtBuilder {
    fn setup_type(&self) -> WyckoffSetupType { WyckoffSetupType::Ut }

    fn try_build(&self, ctx: &SetupContext<'_>) -> Option<WyckoffSetupCandidate> {
        gate_basic(ctx, WyckoffSetupType::Ut)?;
        if !matches!(ctx.tracker.schematic,
            WyckoffSchematic::Distribution | WyckoffSchematic::ReDistribution) { return None; }
        // UTAD has its own builder; here we look for Shakeout/UTAD as upthrust hint
        let ut = ctx.last_event(WyckoffEvent::UTAD)
            .or_else(|| ctx.last_event(WyckoffEvent::Shakeout))?
            .clone();
        let last = ctx.last()?;
        if last.close > ctx.tracker.range_top { return None; }   // failed back inside

        let sl = ut.price + ctx.cfg.ut_buffer_atr * ctx.atr;
        let entry = last.close.min(ctx.tracker.range_top - ctx.atr * 0.1);
        let target_proj = pnf_target(ctx, SetupDirection::Short);
        let tp_targets = vec![ctx.tracker.range_bottom, target_proj];

        Some(WyckoffSetupCandidate {
            setup_type: WyckoffSetupType::Ut,
            direction: SetupDirection::Short,
            phase: ctx.tracker.current_phase,
            schematic: ctx.tracker.schematic,
            range_top: ctx.tracker.range_top,
            range_bottom: ctx.tracker.range_bottom,
            range_height: ctx.range_height(),
            pnf_target: target_proj,
            entry, sl, tp_targets,
            trigger_event: WyckoffEvent::UTAD,
            trigger_bar_index: ut.bar_index,
            trigger_bar_ts_ms: last.ts_ms,
            trigger_price: ut.price,
            climax: ctx.climax_metrics(last),
            atr_at_trigger: ctx.atr,
            raw_score: ut.score.clamp(0.0, 100.0),
        })
    }
}

// =========================================================================
// 5. UTAD — UpThrust After Distribution (terminal Phase C)
// =========================================================================
pub struct UtadBuilder;
impl SetupBuilder for UtadBuilder {
    fn setup_type(&self) -> WyckoffSetupType { WyckoffSetupType::Utad }

    fn try_build(&self, ctx: &SetupContext<'_>) -> Option<WyckoffSetupCandidate> {
        gate_basic(ctx, WyckoffSetupType::Utad)?;
        if !matches!(ctx.tracker.schematic,
            WyckoffSchematic::Distribution | WyckoffSchematic::ReDistribution) { return None; }
        let utad = ctx.last_event(WyckoffEvent::UTAD)?.clone();
        let last = ctx.last()?;
        if last.close >= ctx.tracker.range_top { return None; }

        let sl = utad.price + ctx.cfg.ut_buffer_atr * ctx.atr;
        let entry = last.close;
        let target_proj = pnf_target(ctx, SetupDirection::Short);
        let ice = ctx.tracker.ice.unwrap_or(ctx.tracker.range_bottom);
        let tp_targets = vec![ice, target_proj];

        Some(WyckoffSetupCandidate {
            setup_type: WyckoffSetupType::Utad,
            direction: SetupDirection::Short,
            phase: ctx.tracker.current_phase,
            schematic: ctx.tracker.schematic,
            range_top: ctx.tracker.range_top,
            range_bottom: ctx.tracker.range_bottom,
            range_height: ctx.range_height(),
            pnf_target: target_proj,
            entry, sl, tp_targets,
            trigger_event: WyckoffEvent::UTAD,
            trigger_bar_index: utad.bar_index,
            trigger_bar_ts_ms: last.ts_ms,
            trigger_price: utad.price,
            climax: ctx.climax_metrics(last),
            atr_at_trigger: ctx.atr,
            raw_score: utad.score.clamp(0.0, 100.0),
        })
    }
}

// =========================================================================
// 6. LPSY — Phase D distribution (last-point-of-supply pullback)
// =========================================================================
pub struct LpsyBuilder;
impl SetupBuilder for LpsyBuilder {
    fn setup_type(&self) -> WyckoffSetupType { WyckoffSetupType::Lpsy }

    fn try_build(&self, ctx: &SetupContext<'_>) -> Option<WyckoffSetupCandidate> {
        gate_basic(ctx, WyckoffSetupType::Lpsy)?;
        if !matches!(ctx.tracker.schematic,
            WyckoffSchematic::Distribution | WyckoffSchematic::ReDistribution) { return None; }
        if ctx.tracker.current_phase < WyckoffPhase::D { return None; }

        let lpsy = ctx.last_event(WyckoffEvent::LPSY)?.clone();
        let last = ctx.last()?;
        if last.close >= lpsy.price { return None; }

        let sl = lpsy.price + ctx.cfg.lpsy_buffer_atr * ctx.atr;
        let entry = last.close;
        let target_proj = pnf_target(ctx, SetupDirection::Short);
        let ice = ctx.tracker.ice.unwrap_or(ctx.tracker.range_bottom);
        let tp_targets = vec![ice, ctx.tracker.range_bottom, target_proj];

        Some(WyckoffSetupCandidate {
            setup_type: WyckoffSetupType::Lpsy,
            direction: SetupDirection::Short,
            phase: ctx.tracker.current_phase,
            schematic: ctx.tracker.schematic,
            range_top: ctx.tracker.range_top,
            range_bottom: ctx.tracker.range_bottom,
            range_height: ctx.range_height(),
            pnf_target: target_proj,
            entry, sl, tp_targets,
            trigger_event: WyckoffEvent::LPSY,
            trigger_bar_index: lpsy.bar_index,
            trigger_bar_ts_ms: last.ts_ms,
            trigger_price: lpsy.price,
            climax: ctx.climax_metrics(last),
            atr_at_trigger: ctx.atr,
            raw_score: lpsy.score.clamp(0.0, 100.0),
        })
    }
}

// =========================================================================
// 7. IceRetest — break of ice + retest from below
// =========================================================================
pub struct IceRetestBuilder;
impl SetupBuilder for IceRetestBuilder {
    fn setup_type(&self) -> WyckoffSetupType { WyckoffSetupType::IceRetest }

    fn try_build(&self, ctx: &SetupContext<'_>) -> Option<WyckoffSetupCandidate> {
        gate_basic(ctx, WyckoffSetupType::IceRetest)?;
        if !matches!(ctx.tracker.schematic,
            WyckoffSchematic::Distribution | WyckoffSchematic::ReDistribution) { return None; }
        let bo = ctx.last_event(WyckoffEvent::BreakOfIce)?.clone();
        let last = ctx.last()?;
        let ice = ctx.tracker.ice.unwrap_or(ctx.tracker.range_bottom);
        // Retest: price came back near ice and rejected (close < ice)
        if last.close >= ice { return None; }

        let sl = ice + ctx.cfg.ice_retest_buffer_atr * ctx.atr;
        let entry = last.close;
        let target_proj = pnf_target(ctx, SetupDirection::Short);
        let tp_targets = vec![ctx.tracker.range_bottom, target_proj];

        Some(WyckoffSetupCandidate {
            setup_type: WyckoffSetupType::IceRetest,
            direction: SetupDirection::Short,
            phase: ctx.tracker.current_phase,
            schematic: ctx.tracker.schematic,
            range_top: ctx.tracker.range_top,
            range_bottom: ctx.tracker.range_bottom,
            range_height: ctx.range_height(),
            pnf_target: target_proj,
            entry, sl, tp_targets,
            trigger_event: WyckoffEvent::BreakOfIce,
            trigger_bar_index: bo.bar_index,
            trigger_bar_ts_ms: last.ts_ms,
            trigger_price: bo.price,
            climax: ctx.climax_metrics(last),
            atr_at_trigger: ctx.atr,
            raw_score: bo.score.clamp(0.0, 100.0),
        })
    }
}

// =========================================================================
// Convenience: run all builders, return all matching candidates.
// =========================================================================

pub fn build_all(ctx: &SetupContext<'_>) -> Vec<WyckoffSetupCandidate> {
    all_builders()
        .iter()
        .filter_map(|b| b.try_build(ctx))
        .collect()
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod test {
    use super::*;

    fn mk_bar(ts_ms: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> SetupBar {
        SetupBar { ts_ms, open: o, high: h, low: l, close: c, volume: v }
    }

    #[test]
    fn spring_emits_long_setup_with_levels() {
        let mut tr = WyckoffStructureTracker::new(
            WyckoffSchematic::Accumulation, 100.0, 90.0);
        tr.current_phase = WyckoffPhase::C;
        tr.record_event(WyckoffEvent::SC, 1, 89.0, 80.0);
        tr.record_event(WyckoffEvent::AR, 5, 99.0, 70.0);
        tr.record_event(WyckoffEvent::Spring, 30, 88.0, 85.0);

        let bars: Vec<SetupBar> = (0..30)
            .map(|i| mk_bar(i * 60_000, 92.0, 95.0, 88.0, 93.0, 1000.0))
            .collect();
        let cfg = WyckoffSetupConfig::default();
        let ctx = SetupContext { tracker: &tr, bars: &bars, atr: 2.0, vol_avg_20: 1000.0, cfg: &cfg };

        let cand = SpringBuilder.try_build(&ctx).expect("spring should fire");
        assert_eq!(cand.direction, SetupDirection::Long);
        assert!(cand.sl < cand.entry, "long: sl below entry");
        assert!(cand.tp_targets.iter().all(|t| *t > cand.entry));
        assert!(cand.pnf_target > cand.range_top);
    }

    #[test]
    fn lpsy_emits_short_setup() {
        let mut tr = WyckoffStructureTracker::new(
            WyckoffSchematic::Distribution, 110.0, 100.0);
        tr.current_phase = WyckoffPhase::D;
        tr.record_event(WyckoffEvent::BC, 1, 111.0, 80.0);
        tr.record_event(WyckoffEvent::LPSY, 40, 108.0, 75.0);

        let bars: Vec<SetupBar> = (0..30)
            .map(|i| mk_bar(i * 60_000, 107.0, 109.0, 105.0, 106.0, 1000.0))
            .collect();
        let cfg = WyckoffSetupConfig::default();
        let ctx = SetupContext { tracker: &tr, bars: &bars, atr: 1.5, vol_avg_20: 1000.0, cfg: &cfg };

        let cand = LpsyBuilder.try_build(&ctx).expect("lpsy should fire");
        assert_eq!(cand.direction, SetupDirection::Short);
        assert!(cand.sl > cand.entry);
        assert!(cand.tp_targets.iter().all(|t| *t < cand.entry));
    }

    #[test]
    fn phase_b_blocks_setup() {
        let mut tr = WyckoffStructureTracker::new(
            WyckoffSchematic::Accumulation, 100.0, 90.0);
        tr.current_phase = WyckoffPhase::B;   // too early
        tr.record_event(WyckoffEvent::Spring, 30, 88.0, 85.0);

        let bars: Vec<SetupBar> = (0..30)
            .map(|i| mk_bar(i * 60_000, 92.0, 95.0, 88.0, 93.0, 1000.0))
            .collect();
        let cfg = WyckoffSetupConfig::default();
        let ctx = SetupContext { tracker: &tr, bars: &bars, atr: 2.0, vol_avg_20: 1000.0, cfg: &cfg };
        assert!(SpringBuilder.try_build(&ctx).is_none());
    }
}
