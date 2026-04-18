//! Faz 9.8.7 — SL ratchet full policies.
//!
//! A ladder of [`RatchetPolicy`] impls evaluates a position + an
//! indicator context and proposes a tighter stop-loss. The registry
//! walks every policy; the **most aggressive tightening** wins so a
//! breakeven hand-off cannot accidentally loosen a trailing stop that
//! has already moved past it.
//!
//! CLAUDE.md #1 — policies are trait impls, not nested if/else.
//! CLAUDE.md #2 — thresholds live on [`RatchetConfig`].

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::live_position_store::{LivePositionState, PositionSide};
use crate::scale_manager::{unrealised_r, ScaleContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RatchetKind {
    None,
    Breakeven,
    Trailing,
    Chandelier,
}

#[derive(Debug, Clone)]
pub struct RatchetDecision {
    pub kind: RatchetKind,
    pub new_sl: Option<Decimal>,
}

impl RatchetDecision {
    pub fn none() -> Self {
        Self { kind: RatchetKind::None, new_sl: None }
    }
}

#[derive(Debug, Clone)]
pub struct RatchetConfig {
    pub enabled: bool,
    pub breakeven_trigger_r: f64,
    pub breakeven_offset_pct: f64,
    pub trailing_atr_mult: f64,
    pub chandelier_atr_mult: f64,
    pub chandelier_trigger_r: f64,
}

impl Default for RatchetConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            breakeven_trigger_r: 1.0,
            breakeven_offset_pct: 0.0005,
            trailing_atr_mult: 2.0,
            chandelier_atr_mult: 3.0,
            chandelier_trigger_r: 2.0,
        }
    }
}

pub trait RatchetPolicy: Send + Sync {
    fn kind(&self) -> RatchetKind;
    fn propose(&self, state: &LivePositionState, ctx: &ScaleContext, cfg: &RatchetConfig) -> Option<Decimal>;
}

pub struct BreakevenPolicy;
impl RatchetPolicy for BreakevenPolicy {
    fn kind(&self) -> RatchetKind { RatchetKind::Breakeven }
    fn propose(&self, state: &LivePositionState, ctx: &ScaleContext, cfg: &RatchetConfig) -> Option<Decimal> {
        let r = unrealised_r(state, ctx)?;
        if r < cfg.breakeven_trigger_r { return None; }
        let offset = f64_to_decimal(decimal_to_f64(state.entry_avg) * cfg.breakeven_offset_pct);
        Some(match state.side {
            PositionSide::Buy => state.entry_avg + offset,
            PositionSide::Sell => state.entry_avg - offset,
        })
    }
}

pub struct TrailingAtrPolicy;
impl RatchetPolicy for TrailingAtrPolicy {
    fn kind(&self) -> RatchetKind { RatchetKind::Trailing }
    fn propose(&self, state: &LivePositionState, ctx: &ScaleContext, cfg: &RatchetConfig) -> Option<Decimal> {
        let atr = ctx.atr?;
        let mark = state.last_mark?;
        let dist = f64_to_decimal(decimal_to_f64(atr) * cfg.trailing_atr_mult);
        if dist <= Decimal::ZERO { return None; }
        Some(match state.side {
            PositionSide::Buy => mark - dist,
            PositionSide::Sell => mark + dist,
        })
    }
}

pub struct ChandelierPolicy;
impl RatchetPolicy for ChandelierPolicy {
    fn kind(&self) -> RatchetKind { RatchetKind::Chandelier }
    fn propose(&self, state: &LivePositionState, ctx: &ScaleContext, cfg: &RatchetConfig) -> Option<Decimal> {
        let r = unrealised_r(state, ctx)?;
        if r < cfg.chandelier_trigger_r { return None; }
        let atr = ctx.atr?;
        let best = ctx.best_price?;
        let dist = f64_to_decimal(decimal_to_f64(atr) * cfg.chandelier_atr_mult);
        if dist <= Decimal::ZERO { return None; }
        Some(match state.side {
            PositionSide::Buy => best - dist,
            PositionSide::Sell => best + dist,
        })
    }
}

pub struct RatchetRegistry {
    policies: Vec<Box<dyn RatchetPolicy>>,
}

impl RatchetRegistry {
    pub fn new() -> Self { Self { policies: Vec::new() } }
    pub fn with_defaults() -> Self {
        let mut r = Self::new();
        r.register(Box::new(BreakevenPolicy));
        r.register(Box::new(TrailingAtrPolicy));
        r.register(Box::new(ChandelierPolicy));
        r
    }
    pub fn register(&mut self, p: Box<dyn RatchetPolicy>) { self.policies.push(p); }
    pub fn len(&self) -> usize { self.policies.len() }

    pub fn evaluate(&self, state: &LivePositionState, ctx: &ScaleContext, cfg: &RatchetConfig) -> RatchetDecision {
        if !cfg.enabled { return RatchetDecision::none(); }
        let mut best: Option<(RatchetKind, Decimal)> = None;
        for p in &self.policies {
            let Some(candidate) = p.propose(state, ctx, cfg) else { continue };
            let Some(accepted) = tighten_only(state, candidate) else { continue };
            best = Some(match best {
                None => (p.kind(), accepted),
                Some((k, cur)) => if tighter_than(state, accepted, cur) {
                    (p.kind(), accepted)
                } else { (k, cur) },
            });
        }
        match best {
            Some((k, sl)) => RatchetDecision { kind: k, new_sl: Some(sl) },
            None => RatchetDecision::none(),
        }
    }
}

impl Default for RatchetRegistry {
    fn default() -> Self { Self::with_defaults() }
}

pub fn tighten_only(state: &LivePositionState, new_sl: Decimal) -> Option<Decimal> {
    let current = state.current_sl?;
    let tighter = match state.side {
        PositionSide::Buy => new_sl > current,
        PositionSide::Sell => new_sl < current,
    };
    tighter.then_some(new_sl)
}

fn tighter_than(state: &LivePositionState, a: Decimal, b: Decimal) -> bool {
    match state.side {
        PositionSide::Buy => a > b,
        PositionSide::Sell => a < b,
    }
}

pub fn evaluate(_state: &LivePositionState) -> RatchetDecision {
    RatchetDecision::none()
}

pub fn evaluate_with_context(state: &LivePositionState, ctx: &ScaleContext, cfg: &RatchetConfig) -> RatchetDecision {
    RatchetRegistry::with_defaults().evaluate(state, ctx, cfg)
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

    fn long_state(mark: Decimal, sl: Decimal) -> LivePositionState {
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
            current_sl: Some(sl),
            tp_ladder: Vec::new(),
            liquidation_price: None,
            maint_margin_ratio: None,
            funding_rate_next: None,
            last_mark: Some(mark),
            last_tick_at: Some(Utc::now()),
            opened_at: Utc::now(),
        }
    }

    fn ctx() -> ScaleContext {
        ScaleContext {
            atr: Some(dec!(2)),
            initial_risk_per_unit: Some(dec!(2)),
            best_price: Some(dec!(110)),
            ..Default::default()
        }
    }

    #[test]
    fn breakeven_fires_at_1r() {
        let out = evaluate_with_context(&long_state(dec!(102), dec!(98)), &ctx(), &RatchetConfig::default());
        assert_ne!(out.kind, RatchetKind::None);
        let sl = out.new_sl.unwrap();
        assert!(sl > dec!(98) && sl >= dec!(100));
    }

    #[test]
    fn tighten_only_rejects_looser() {
        let s = long_state(dec!(102), dec!(99));
        assert!(tighten_only(&s, dec!(98)).is_none());
        assert_eq!(tighten_only(&s, dec!(100)), Some(dec!(100)));
    }

    #[test]
    fn short_breakeven_flips_sign() {
        let mut s = long_state(dec!(98), dec!(102));
        s.side = PositionSide::Sell;
        let out = evaluate_with_context(&s, &ctx(), &RatchetConfig::default());
        let sl = out.new_sl.unwrap();
        assert!(sl < dec!(102) && sl <= dec!(100));
    }

    #[test]
    fn disabled_returns_none() {
        let cfg = RatchetConfig { enabled: false, ..Default::default() };
        let out = evaluate_with_context(&long_state(dec!(110), dec!(98)), &ctx(), &cfg);
        assert_eq!(out.kind, RatchetKind::None);
    }

    #[test]
    fn no_match_when_below_all_triggers() {
        // mark = entry — no R gained, trailing would propose 96, looser than 98.
        let out = evaluate_with_context(&long_state(dec!(100), dec!(98)), &ctx(), &RatchetConfig::default());
        assert_eq!(out.kind, RatchetKind::None);
    }

    #[test]
    fn high_profit_picks_tightest_of_all() {
        // mark 110, best 110, atr 2 → trailing=106, chandelier=104, breakeven=100.05
        // Tightest (highest for long) = trailing 106.
        let out = evaluate_with_context(&long_state(dec!(110), dec!(98)), &ctx(), &RatchetConfig::default());
        assert_eq!(out.new_sl, Some(dec!(106)));
        assert_eq!(out.kind, RatchetKind::Trailing);
    }
}
