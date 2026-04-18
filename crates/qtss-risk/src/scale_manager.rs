//! Faz 9.8.6 — Scale manager full logic.
//!
//! Dispatches a ladder of [`ScaleRule`] impls against a live position
//! + a caller-supplied [`ScaleContext`] (ATR, initial risk, realised
//! R-multiple, pyramid leg count). First rule that matches wins;
//! returns a concrete [`ScaleDecision`] the execution layer can act on.
//!
//! CLAUDE.md #1 — rules are trait impls in a `Vec`, not nested if/else.
//! CLAUDE.md #2 — every threshold sits on [`ScaleManagerConfig`] and
//! is populated from `qtss_config` by the worker loader.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::live_position_store::{LivePositionState, PositionSide};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleDecisionKind {
    Hold,
    PyramidIn,
    ScaleOut,
    AddOnDip,
    PartialTp,
}

#[derive(Debug, Clone)]
pub struct ScaleDecision {
    pub kind: ScaleDecisionKind,
    /// Positive = add to position, negative = reduce.
    pub qty_delta: Decimal,
    pub reason: &'static str,
}

impl ScaleDecision {
    pub fn hold() -> Self {
        Self {
            kind: ScaleDecisionKind::Hold,
            qty_delta: Decimal::ZERO,
            reason: "no scale condition met",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScaleManagerConfig {
    pub enabled: bool,
    pub max_pyramid_legs: u8,
    pub pyramid_trigger_r: f64,
    pub pyramid_leg_pct: f64,
    pub scale_out_r: f64,
    pub scale_out_pct: f64,
    pub add_on_dip_atr_mult: f64,
    pub add_on_dip_pct: f64,
}

impl Default for ScaleManagerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_pyramid_legs: 2,
            pyramid_trigger_r: 1.0,
            pyramid_leg_pct: 0.5,
            scale_out_r: 2.0,
            scale_out_pct: 0.33,
            add_on_dip_atr_mult: 1.0,
            add_on_dip_pct: 0.25,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScaleContext {
    pub atr: Option<Decimal>,
    pub initial_risk_per_unit: Option<Decimal>,
    pub pyramid_legs_added: u8,
    pub scale_out_milestones_hit: u8,
    pub best_price: Option<Decimal>,
}

pub fn unrealised_r(state: &LivePositionState, ctx: &ScaleContext) -> Option<f64> {
    let mark = state.last_mark?;
    let risk = ctx.initial_risk_per_unit?;
    if risk <= Decimal::ZERO {
        return None;
    }
    let diff = match state.side {
        PositionSide::Buy => mark - state.entry_avg,
        PositionSide::Sell => state.entry_avg - mark,
    };
    (diff / risk).to_f64()
}

pub trait ScaleRule: Send + Sync {
    fn tag(&self) -> &'static str;
    fn evaluate(
        &self,
        state: &LivePositionState,
        ctx: &ScaleContext,
        cfg: &ScaleManagerConfig,
    ) -> Option<ScaleDecision>;
}

pub struct PyramidInRule;
impl ScaleRule for PyramidInRule {
    fn tag(&self) -> &'static str { "pyramid_in" }
    fn evaluate(&self, state: &LivePositionState, ctx: &ScaleContext, cfg: &ScaleManagerConfig) -> Option<ScaleDecision> {
        if ctx.pyramid_legs_added >= cfg.max_pyramid_legs { return None; }
        let r = unrealised_r(state, ctx)?;
        if r < cfg.pyramid_trigger_r { return None; }
        let add = f64_to_decimal(decimal_to_f64(state.qty_remaining) * cfg.pyramid_leg_pct);
        if add <= Decimal::ZERO { return None; }
        Some(ScaleDecision { kind: ScaleDecisionKind::PyramidIn, qty_delta: add, reason: "unrealised R above pyramid trigger" })
    }
}

pub struct ScaleOutRule;
impl ScaleRule for ScaleOutRule {
    fn tag(&self) -> &'static str { "scale_out" }
    fn evaluate(&self, state: &LivePositionState, ctx: &ScaleContext, cfg: &ScaleManagerConfig) -> Option<ScaleDecision> {
        let r = unrealised_r(state, ctx)?;
        let due = (r / cfg.scale_out_r).floor() as i64;
        if due <= ctx.scale_out_milestones_hit as i64 { return None; }
        let reduce = f64_to_decimal(decimal_to_f64(state.qty_remaining) * cfg.scale_out_pct);
        if reduce <= Decimal::ZERO { return None; }
        Some(ScaleDecision { kind: ScaleDecisionKind::ScaleOut, qty_delta: -reduce, reason: "scale-out milestone reached" })
    }
}

pub struct AddOnDipRule;
impl ScaleRule for AddOnDipRule {
    fn tag(&self) -> &'static str { "add_on_dip" }
    fn evaluate(&self, state: &LivePositionState, ctx: &ScaleContext, cfg: &ScaleManagerConfig) -> Option<ScaleDecision> {
        let atr = ctx.atr?;
        if atr <= Decimal::ZERO { return None; }
        let mark = state.last_mark?;
        let best = ctx.best_price?;
        let dip = match state.side {
            PositionSide::Buy if best > mark => best - mark,
            PositionSide::Sell if best < mark => mark - best,
            _ => return None,
        };
        if decimal_to_f64(dip) / decimal_to_f64(atr) < cfg.add_on_dip_atr_mult { return None; }
        let still_favourable = match state.side {
            PositionSide::Buy => mark > state.entry_avg,
            PositionSide::Sell => mark < state.entry_avg,
        };
        if !still_favourable { return None; }
        let add = f64_to_decimal(decimal_to_f64(state.qty_remaining) * cfg.add_on_dip_pct);
        if add <= Decimal::ZERO { return None; }
        Some(ScaleDecision { kind: ScaleDecisionKind::AddOnDip, qty_delta: add, reason: "pullback exceeded ATR threshold while still favourable" })
    }
}

pub struct ScaleRuleRegistry {
    rules: Vec<Box<dyn ScaleRule>>,
}

impl ScaleRuleRegistry {
    pub fn new() -> Self { Self { rules: Vec::new() } }
    pub fn with_defaults() -> Self {
        let mut r = Self::new();
        r.register(Box::new(ScaleOutRule));
        r.register(Box::new(PyramidInRule));
        r.register(Box::new(AddOnDipRule));
        r
    }
    pub fn register(&mut self, r: Box<dyn ScaleRule>) { self.rules.push(r); }
    pub fn len(&self) -> usize { self.rules.len() }
    pub fn evaluate(&self, state: &LivePositionState, ctx: &ScaleContext, cfg: &ScaleManagerConfig) -> ScaleDecision {
        if !cfg.enabled { return ScaleDecision::hold(); }
        for r in &self.rules {
            if let Some(d) = r.evaluate(state, ctx, cfg) { return d; }
        }
        ScaleDecision::hold()
    }
}

impl Default for ScaleRuleRegistry {
    fn default() -> Self { Self::with_defaults() }
}

/// Back-compat entry — without a `ScaleContext` every rule abstains,
/// so we return Hold. Tick dispatcher (9.8.5) calls this; worker
/// upgrade to `evaluate_with_context` happens alongside indicator-
/// store hydration in 9.8.7.
pub fn evaluate(_state: &LivePositionState, _cfg: &ScaleManagerConfig) -> ScaleDecision {
    ScaleDecision::hold()
}

pub fn evaluate_with_context(state: &LivePositionState, ctx: &ScaleContext, cfg: &ScaleManagerConfig) -> ScaleDecision {
    ScaleRuleRegistry::with_defaults().evaluate(state, ctx, cfg)
}

fn decimal_to_f64(d: Decimal) -> f64 { d.to_f64().unwrap_or(0.0) }

fn f64_to_decimal(f: f64) -> Decimal {
    use std::str::FromStr;
    if !f.is_finite() { return Decimal::ZERO; }
    Decimal::from_str(&format!("{f:.8}")).unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_position_store::{ExecutionMode, MarketSegment};
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn long_state(mark: Decimal) -> LivePositionState {
        LivePositionState {
            id: Uuid::new_v4(),
            setup_id: None,
            mode: ExecutionMode::Dry,
            exchange: "binance".into(),
            segment: MarketSegment::Futures,
            symbol: "BTCUSDT".into(),
            side: PositionSide::Buy,
            leverage: 10,
            entry_avg: dec!(100),
            qty_filled: dec!(1),
            qty_remaining: dec!(1),
            current_sl: Some(dec!(98)),
            tp_ladder: Vec::new(),
            liquidation_price: None,
            maint_margin_ratio: None,
            funding_rate_next: None,
            last_mark: Some(mark),
            last_tick_at: Some(Utc::now()),
            opened_at: Utc::now(),
        }
    }

    fn base_ctx() -> ScaleContext {
        ScaleContext {
            atr: Some(dec!(2)),
            initial_risk_per_unit: Some(dec!(2)),
            pyramid_legs_added: 0,
            scale_out_milestones_hit: 0,
            best_price: Some(dec!(104)),
        }
    }

    #[test]
    fn hold_when_disabled() {
        let cfg = ScaleManagerConfig { enabled: false, ..Default::default() };
        let out = evaluate_with_context(&long_state(dec!(110)), &base_ctx(), &cfg);
        assert_eq!(out.kind, ScaleDecisionKind::Hold);
    }

    #[test]
    fn pyramid_fires_above_trigger() {
        let out = evaluate_with_context(&long_state(dec!(102)), &base_ctx(), &ScaleManagerConfig::default());
        assert_eq!(out.kind, ScaleDecisionKind::PyramidIn);
    }

    #[test]
    fn scale_out_fires_at_2r() {
        let out = evaluate_with_context(&long_state(dec!(104)), &base_ctx(), &ScaleManagerConfig::default());
        assert_eq!(out.kind, ScaleDecisionKind::ScaleOut);
        assert!(out.qty_delta < Decimal::ZERO);
    }

    #[test]
    fn scale_out_suppressed_after_hit() {
        let mut ctx = base_ctx();
        ctx.scale_out_milestones_hit = 1;
        let out = evaluate_with_context(&long_state(dec!(104)), &ctx, &ScaleManagerConfig::default());
        assert_eq!(out.kind, ScaleDecisionKind::PyramidIn);
    }

    #[test]
    fn pyramid_respects_cap() {
        let mut ctx = base_ctx();
        ctx.pyramid_legs_added = 2;
        let out = evaluate_with_context(&long_state(dec!(103)), &ctx, &ScaleManagerConfig::default());
        assert_eq!(out.kind, ScaleDecisionKind::Hold);
    }

    #[test]
    fn add_on_dip_fires_when_pullback_ok_and_in_profit() {
        let mut ctx = base_ctx();
        ctx.initial_risk_per_unit = Some(dec!(10));
        ctx.best_price = Some(dec!(104));
        let out = evaluate_with_context(&long_state(dec!(101)), &ctx, &ScaleManagerConfig::default());
        assert_eq!(out.kind, ScaleDecisionKind::AddOnDip);
    }

    #[test]
    fn dip_skipped_when_below_entry() {
        let mut ctx = base_ctx();
        ctx.initial_risk_per_unit = Some(dec!(10));
        ctx.best_price = Some(dec!(104));
        let out = evaluate_with_context(&long_state(dec!(99)), &ctx, &ScaleManagerConfig::default());
        assert_eq!(out.kind, ScaleDecisionKind::Hold);
    }

    #[test]
    fn short_pyramid_uses_inverse_sign() {
        let mut s = long_state(dec!(98));
        s.side = PositionSide::Sell;
        let out = evaluate_with_context(&s, &base_ctx(), &ScaleManagerConfig::default());
        assert_eq!(out.kind, ScaleDecisionKind::PyramidIn);
    }
}
