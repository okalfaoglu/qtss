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
    Jac,
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
            Self::Jac => "wyckoff_jac",
            Self::Ut => "wyckoff_ut",
            Self::Utad => "wyckoff_utad",
            Self::Lpsy => "wyckoff_lpsy",
            Self::IceRetest => "wyckoff_ice_retest",
        }
    }

    pub fn direction(self) -> SetupDirection {
        match self {
            Self::Spring | Self::Lps | Self::Buec | Self::Jac => SetupDirection::Long,
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
    /// P7.3 — ATR buffer past the structural range boundary used to
    /// compute the **wide** invalidation stop. Long → range_bottom −
    /// buffer·ATR; short → range_top + buffer·ATR. The close-to-trigger
    /// "tight" stop remains each setup's classical `*_buffer_atr`.
    pub sl_wide_buffer_atr: f64,
    /// P7.5 — directional trigger-bar gate. Long setups require a
    /// bullish close (close > open) with close position in the upper
    /// `trigger_bar_min_close_pos` of the bar range; short setups
    /// require the mirror. A "close in the upper third with volume"
    /// trigger bar is the canonical Wyckoff commitment signal
    /// (Villahermosa §7.4.1). Set `require_directional_trigger=false`
    /// to disable (for backtests / legacy behaviour).
    pub require_directional_trigger: bool,
    pub trigger_bar_min_close_pos: f64,
    /// P7.5 — JAC (Jump Across Creek) setup: wide-range bullish bar
    /// breaks the creek with volume ≥ `jac_min_volume_ratio × vol_avg_20`.
    pub jac_min_volume_ratio: f64,
    pub jac_min_range_atr: f64,
    pub jac_buffer_atr: f64,
    /// Villahermosa *Wyckoff 2.0* §7.4.2 — Phase-D continuation setups
    /// (LPS/BUEC/LPSY/IceRetest) must be preceded by a fresh SOS (long)
    /// or SOW (short) confirming that Composite Operator is driving
    /// price in the setup direction. Without the SOS/SOW the "LPS" is
    /// just a random pullback. Phase-C climax setups (Spring/UT/UTAD)
    /// are exempt — they ARE the initial trigger.
    pub require_sos_sow_trigger: bool,
    /// Max bars between the SOS/SOW and the setup trigger for the
    /// confirmation to still count.
    pub sos_sow_max_bars_ago: u64,
    /// Setup types that require the SOS/SOW gate. Others bypass it.
    pub sos_sow_required_for: Vec<WyckoffSetupType>,
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
                WyckoffSetupType::Jac,
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
            sl_wide_buffer_atr: 0.5,
            require_directional_trigger: true,
            trigger_bar_min_close_pos: 0.5,
            jac_min_volume_ratio: 1.5,
            jac_min_range_atr: 1.2,
            jac_buffer_atr: 0.4,
            require_sos_sow_trigger: true,
            sos_sow_max_bars_ago: 50,
            sos_sow_required_for: vec![
                WyckoffSetupType::Lps,
                WyckoffSetupType::Buec,
                WyckoffSetupType::Lpsy,
                WyckoffSetupType::IceRetest,
            ],
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
    /// Classical "tight" SL — close to the trigger bar (spring low /
    /// UT high ± buffer). Used for R:R math.
    pub sl: f64,
    /// Structural "wide" SL — below/above the range boundary ±
    /// `sl_wide_buffer_atr·ATR`. Full invalidation of the setup
    /// hypothesis; trade planner may elect this instead of `sl`.
    pub sl_wide: f64,
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
        Box::new(JacBuilder),
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

/// P7.2 — SOS/SOW confirmation gate. Phase-D continuation setups
/// require a Sign-of-Strength (long) or Sign-of-Weakness (short) to
/// have fired within `sos_sow_max_bars_ago` bars BEFORE the setup
/// trigger. Without that confirmation the pullback is not a genuine
/// LPS/BUEC/LPSY/IceRetest — just a regime-neutral retracement.
fn gate_sos_sow(
    ctx: &SetupContext<'_>,
    ty: WyckoffSetupType,
    dir: SetupDirection,
    trigger_bar: u64,
) -> Option<()> {
    if !ctx.cfg.require_sos_sow_trigger { return Some(()); }
    if !ctx.cfg.sos_sow_required_for.contains(&ty) { return Some(()); }
    let needed = match dir {
        SetupDirection::Long => WyckoffEvent::SOS,
        SetupDirection::Short => WyckoffEvent::SOW,
    };
    let hit = ctx.tracker.events.iter().rev().find(|e| e.event == needed)?;
    if hit.bar_index > trigger_bar { return None; }
    if trigger_bar.saturating_sub(hit.bar_index) > ctx.cfg.sos_sow_max_bars_ago {
        return None;
    }
    Some(())
}

/// P7.5 — directional trigger-bar gate. Long setups need a bullish bar
/// (close > open) whose close sits in the upper `min_close_pos` of the
/// bar range; short setups need the mirror. Disabled via
/// `require_directional_trigger=false`.
fn gate_trigger_bar(ctx: &SetupContext<'_>, dir: SetupDirection, bar: &SetupBar) -> Option<()> {
    if !ctx.cfg.require_directional_trigger { return Some(()); }
    let rng = (bar.high - bar.low).max(1e-9);
    let min_pos = ctx.cfg.trigger_bar_min_close_pos.clamp(0.0, 1.0);
    let ok = match dir {
        SetupDirection::Long  => bar.close > bar.open && (bar.close - bar.low)  / rng >= min_pos,
        SetupDirection::Short => bar.close < bar.open && (bar.high - bar.close) / rng >= min_pos,
    };
    if ok { Some(()) } else { None }
}

/// Structural "wide" SL — below range_bottom for longs, above
/// range_top for shorts, with an ATR buffer (`sl_wide_buffer_atr`).
fn structural_sl(ctx: &SetupContext<'_>, dir: SetupDirection) -> f64 {
    let buf = ctx.cfg.sl_wide_buffer_atr * ctx.atr;
    match dir {
        SetupDirection::Long  => ctx.tracker.range_bottom - buf,
        SetupDirection::Short => ctx.tracker.range_top + buf,
    }
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
        gate_trigger_bar(ctx, SetupDirection::Long, last)?;

        let sl = spring.price - ctx.cfg.spring_buffer_atr * ctx.atr;
        let sl_wide = structural_sl(ctx, SetupDirection::Long);
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
            sl_wide,
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
        gate_trigger_bar(ctx, SetupDirection::Long, last)?;
        gate_sos_sow(ctx, WyckoffSetupType::Lps, SetupDirection::Long, lps.bar_index)?;

        let sl = lps.price - ctx.cfg.lps_buffer_atr * ctx.atr;
        let sl_wide = structural_sl(ctx, SetupDirection::Long);
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
            sl_wide,
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
        gate_trigger_bar(ctx, SetupDirection::Long, last)?;
        gate_sos_sow(ctx, WyckoffSetupType::Buec, SetupDirection::Long, buec.bar_index)?;

        let sl = creek - ctx.cfg.buec_buffer_atr * ctx.atr;
        let sl_wide = structural_sl(ctx, SetupDirection::Long);
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
            entry, sl, sl_wide, tp_targets,
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
        gate_trigger_bar(ctx, SetupDirection::Short, last)?;

        let sl = ut.price + ctx.cfg.ut_buffer_atr * ctx.atr;
        let sl_wide = structural_sl(ctx, SetupDirection::Short);
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
            entry, sl, sl_wide, tp_targets,
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
        gate_trigger_bar(ctx, SetupDirection::Short, last)?;

        let sl = utad.price + ctx.cfg.ut_buffer_atr * ctx.atr;
        let sl_wide = structural_sl(ctx, SetupDirection::Short);
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
            entry, sl, sl_wide, tp_targets,
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
        gate_trigger_bar(ctx, SetupDirection::Short, last)?;
        gate_sos_sow(ctx, WyckoffSetupType::Lpsy, SetupDirection::Short, lpsy.bar_index)?;

        let sl = lpsy.price + ctx.cfg.lpsy_buffer_atr * ctx.atr;
        let sl_wide = structural_sl(ctx, SetupDirection::Short);
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
            entry, sl, sl_wide, tp_targets,
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
        gate_trigger_bar(ctx, SetupDirection::Short, last)?;
        gate_sos_sow(ctx, WyckoffSetupType::IceRetest, SetupDirection::Short, bo.bar_index)?;

        let sl = ice + ctx.cfg.ice_retest_buffer_atr * ctx.atr;
        let sl_wide = structural_sl(ctx, SetupDirection::Short);
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
            entry, sl, sl_wide, tp_targets,
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
// 8. JAC — Jump Across the Creek (Phase D accumulation breakout)
//
// Villahermosa §7.4.1: a wide-range bullish bar that closes decisively above
// the creek on above-average volume. Distinct from BUEC (which is the *retest*
// after the break) — JAC fires on the breakout bar itself.
// =========================================================================
pub struct JacBuilder;
impl SetupBuilder for JacBuilder {
    fn setup_type(&self) -> WyckoffSetupType { WyckoffSetupType::Jac }

    fn try_build(&self, ctx: &SetupContext<'_>) -> Option<WyckoffSetupCandidate> {
        gate_basic(ctx, WyckoffSetupType::Jac)?;
        if !matches!(ctx.tracker.schematic,
            WyckoffSchematic::Accumulation | WyckoffSchematic::ReAccumulation) { return None; }
        if ctx.tracker.current_phase < WyckoffPhase::D { return None; }

        let jac = ctx.last_event(WyckoffEvent::JAC)?.clone();
        let last = ctx.last()?;
        let creek = ctx.tracker.creek.unwrap_or(ctx.tracker.range_top);

        // Breakout bar must close above the creek.
        if last.close <= creek { return None; }

        // Volume filter: breakout bar must be ≥ jac_min_volume_ratio × vol_avg_20.
        if ctx.vol_avg_20 > 0.0 {
            let vr = last.volume / ctx.vol_avg_20;
            if vr < ctx.cfg.jac_min_volume_ratio { return None; }
        }
        // Range filter: wide bar per Villahermosa.
        if ctx.atr > 0.0 {
            let rng = last.high - last.low;
            if rng < ctx.cfg.jac_min_range_atr * ctx.atr { return None; }
        }

        gate_trigger_bar(ctx, SetupDirection::Long, last)?;
        gate_sos_sow(ctx, WyckoffSetupType::Jac, SetupDirection::Long, jac.bar_index)?;

        let sl = creek - ctx.cfg.jac_buffer_atr * ctx.atr;
        let sl_wide = structural_sl(ctx, SetupDirection::Long);
        let entry = last.close;
        let target_proj = pnf_target(ctx, SetupDirection::Long);
        let tp_targets = vec![ctx.tracker.range_top, target_proj];

        Some(WyckoffSetupCandidate {
            setup_type: WyckoffSetupType::Jac,
            direction: SetupDirection::Long,
            phase: ctx.tracker.current_phase,
            schematic: ctx.tracker.schematic,
            range_top: ctx.tracker.range_top,
            range_bottom: ctx.tracker.range_bottom,
            range_height: ctx.range_height(),
            pnf_target: target_proj,
            entry, sl, sl_wide, tp_targets,
            trigger_event: WyckoffEvent::JAC,
            trigger_bar_index: jac.bar_index,
            trigger_bar_ts_ms: last.ts_ms,
            trigger_price: jac.price,
            climax: ctx.climax_metrics(last),
            atr_at_trigger: ctx.atr,
            raw_score: jac.score.clamp(0.0, 100.0),
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
    fn lps_requires_prior_sos() {
        let mut tr = WyckoffStructureTracker::new(
            WyckoffSchematic::Accumulation, 100.0, 90.0);
        tr.current_phase = WyckoffPhase::D;
        tr.record_event(WyckoffEvent::SC, 1, 89.0, 80.0);
        tr.record_event(WyckoffEvent::AR, 5, 99.0, 70.0);
        tr.record_event(WyckoffEvent::LPS, 40, 93.0, 75.0);

        let bars: Vec<SetupBar> = (0..30)
            .map(|i| mk_bar(i * 60_000, 94.0, 96.0, 92.0, 95.0, 1000.0))
            .collect();
        let cfg = WyckoffSetupConfig::default();
        let ctx = SetupContext { tracker: &tr, bars: &bars, atr: 1.0, vol_avg_20: 1000.0, cfg: &cfg };
        // No SOS yet — gate must block.
        assert!(LpsBuilder.try_build(&ctx).is_none(), "LPS must require SOS");

        // Add an SOS within the window — gate must pass.
        let mut tr2 = tr.clone();
        tr2.record_event(WyckoffEvent::SOS, 35, 97.0, 85.0);
        let ctx2 = SetupContext { tracker: &tr2, bars: &bars, atr: 1.0, vol_avg_20: 1000.0, cfg: &cfg };
        assert!(LpsBuilder.try_build(&ctx2).is_some(), "LPS should fire after SOS");
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
