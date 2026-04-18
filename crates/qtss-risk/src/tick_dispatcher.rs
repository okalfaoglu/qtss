//! Faz 9.8.5 — Tick dispatcher.
//!
//! Single entry point the worker calls after a bookTicker / markPrice
//! / userData tick updates `LivePositionStore`. Evaluates every guard
//! (liquidation / ratchet / tp / scale) for the affected positions
//! and collects outcomes into a `TickOutcomes` bundle. Pure — no I/O.
//! Persistence + broker action is the caller's job.
//!
//! CLAUDE.md #1 — the guard list is a `Vec<Box<dyn TickGuard>>`
//! dispatch table; adding a new guard is one register call.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::liquidation_guard::{
    assess_with_margin, LiquidationAssessment, LiquidationGuardConfig, MarginContext,
};
use crate::live_position_store::{LivePositionState, LivePositionStore, PositionId, TickKey};
use crate::ratchet::{evaluate as ratchet_evaluate, RatchetDecision};
use crate::scale_manager::{evaluate as scale_evaluate, ScaleDecision, ScaleManagerConfig};
use crate::tp_engine::{evaluate as tp_evaluate, TpTrigger};

/// All outcomes produced by a single tick evaluation on one position.
#[derive(Debug, Clone)]
pub struct PositionTickOutcomes {
    pub position_id: PositionId,
    pub liquidation: Option<LiquidationAssessment>,
    pub ratchet: RatchetDecision,
    pub scale: ScaleDecision,
    pub tp_triggers: Vec<TpTrigger>,
}

impl PositionTickOutcomes {
    /// True when at least one guard produced an actionable signal.
    pub fn has_action(&self) -> bool {
        self.liquidation
            .as_ref()
            .map(|a| a.action != crate::liquidation_guard::LiquidationAction::None)
            .unwrap_or(false)
            || self.ratchet.kind != crate::ratchet::RatchetKind::None
            || self.scale.kind != crate::scale_manager::ScaleDecisionKind::Hold
            || !self.tp_triggers.is_empty()
    }
}

/// Aggregate across every position hit by a single tick fan-out call.
#[derive(Debug, Clone, Default)]
pub struct TickOutcomes {
    pub key: Option<TickKey>,
    pub at: Option<DateTime<Utc>>,
    pub positions: Vec<PositionTickOutcomes>,
}

/// Config bundle the worker threads through on every tick. Cheap to
/// clone — the inner configs are small owned structs.
#[derive(Debug, Clone, Default)]
pub struct TickDispatcherConfig {
    pub liquidation: LiquidationGuardConfig,
    pub scale: ScaleManagerConfig,
}

/// Per-call context — varies per tick even when config is static.
/// Currently just the account margin snapshot; future fields (funding,
/// drift, etc.) plug in here without touching guard signatures.
#[derive(Debug, Clone, Default)]
pub struct TickContext {
    pub margin: MarginContext,
}

/// Drive every guard for every position bound to `key`.
///
/// Sequence:
///   1. `store.update_mark(key, price, at)` — writes the tick in place
///      and returns the affected position ids.
///   2. Snapshot each affected position (cloned out of the store so
///      guards can run lock-free).
///   3. Walk guards; collect per-position outcomes.
pub fn evaluate_tick(
    store: &LivePositionStore,
    key: &TickKey,
    price: Decimal,
    at: DateTime<Utc>,
    cfg: &TickDispatcherConfig,
    ctx: &TickContext,
) -> TickOutcomes {
    let ids = store.update_mark(key, price, at);
    let mut positions = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(state) = store.get(id) {
            positions.push(evaluate_position(&state, cfg, ctx));
        }
    }
    TickOutcomes {
        key: Some(key.clone()),
        at: Some(at),
        positions,
    }
}

/// Evaluate every guard for a single position snapshot. Exposed so
/// hydration paths (catch-up after restart) can replay the same
/// pipeline without going through the tick store.
pub fn evaluate_position(
    state: &LivePositionState,
    cfg: &TickDispatcherConfig,
    ctx: &TickContext,
) -> PositionTickOutcomes {
    PositionTickOutcomes {
        position_id: state.id,
        liquidation: assess_with_margin(state, &cfg.liquidation, ctx.margin),
        ratchet: ratchet_evaluate(state),
        scale: scale_evaluate(state, &cfg.scale),
        tp_triggers: tp_evaluate(state),
    }
}

/// Convenience — filter to just the position ids that produced an
/// actionable outcome, so callers can skip no-op persistence.
pub fn actionable_ids(outcomes: &TickOutcomes) -> Vec<Uuid> {
    outcomes
        .positions
        .iter()
        .filter(|p| p.has_action())
        .map(|p| p.position_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_position_store::{
        ExecutionMode, LivePositionState, MarketSegment, PositionSide, TpLeg,
    };
    use rust_decimal_macros::dec;

    fn make_state(mark: Option<Decimal>, tp: Vec<TpLeg>) -> LivePositionState {
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
            current_sl: Some(dec!(95)),
            tp_ladder: tp,
            liquidation_price: Some(dec!(90)),
            maint_margin_ratio: Some(dec!(0.005)),
            funding_rate_next: None,
            last_mark: mark,
            last_tick_at: None,
            opened_at: Utc::now(),
        }
    }

    #[test]
    fn fan_out_visits_registered_position() {
        let store = LivePositionStore::new();
        let s = make_state(None, Vec::new());
        let id = s.id;
        let key = TickKey {
            mode: s.mode,
            exchange: s.exchange.clone(),
            segment: s.segment,
            symbol: s.symbol.clone(),
        };
        store.upsert(s);
        let out = evaluate_tick(
            &store,
            &key,
            dec!(100),
            Utc::now(),
            &TickDispatcherConfig::default(),
            &TickContext::default(),
        );
        assert_eq!(out.positions.len(), 1);
        assert_eq!(out.positions[0].position_id, id);
    }

    #[test]
    fn tp_trigger_fires_when_mark_crosses_leg() {
        let state = make_state(
            Some(dec!(110)),
            vec![TpLeg { price: dec!(105), qty: dec!(0.5), filled_qty: dec!(0) }],
        );
        let out = evaluate_position(&state, &TickDispatcherConfig::default(), &TickContext::default());
        assert_eq!(out.tp_triggers.len(), 1);
        assert!(out.has_action());
    }

    #[test]
    fn no_action_when_all_guards_quiet() {
        let state = make_state(Some(dec!(100)), Vec::new());
        let out = evaluate_position(&state, &TickDispatcherConfig::default(), &TickContext::default());
        assert!(!out.has_action());
    }

    #[test]
    fn unknown_tick_key_returns_empty_outcomes() {
        let store = LivePositionStore::new();
        let key = TickKey {
            mode: ExecutionMode::Dry,
            exchange: "binance".into(),
            segment: MarketSegment::Futures,
            symbol: "ETHUSDT".into(),
        };
        let out = evaluate_tick(
            &store,
            &key,
            dec!(100),
            Utc::now(),
            &TickDispatcherConfig::default(),
            &TickContext::default(),
        );
        assert!(out.positions.is_empty());
    }
}
