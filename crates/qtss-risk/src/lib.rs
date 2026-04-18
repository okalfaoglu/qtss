//! qtss-risk — pre-trade risk gating and position sizing.
//!
//! Sits between strategies and execution: takes a `TradeIntent`, walks
//! a configurable battery of checks, sizes the position, re-applies the
//! leverage cap, and emits an `ApprovedIntent` (or a `RiskRejection`).
//! The execution layer only consumes `ApprovedIntent`, so risk approval
//! is structurally unbypassable.
//!
//! ## Checks
//!
//! Checks dispatch through the [`RiskCheck`] trait. The engine walks
//! every registered check in order; the first rejection short-circuits.
//! Adding a new check is one impl + one register call (CLAUDE.md rule
//! #1). Built-in set:
//!
//! - `KillSwitchCheck`        — manual flag or drawdown >= killswitch
//! - `DrawdownCheck`          — session drawdown >= cap
//! - `DailyLossCheck`         — day pnl loss >= cap
//! - `MaxOpenPositionsCheck`  — open positions >= cap
//! - `LeverageCheck`          — current leverage > cap
//! - `StopDistanceCheck`      — stop loss != entry
//!
//! ## Sizing
//!
//! Each `SizingHint` variant maps to a [`Sizer`] in a `SizerRegistry`
//! `HashMap`, dispatched by tag string. New flavour = one register call.
//! `RiskPctSizer` is the structural reference; `VolTargetSizer` and
//! `KellySizer` currently fall back to the per-trade-risk cap until
//! ATR / hit-rate inputs are wired in by later phases.
//!
//! All thresholds live in [`RiskConfig`] and are validated up-front
//! (CLAUDE.md rule #2). Mirrors the keys seeded in migration 0016.

mod checks;
mod config;
mod engine;
mod error;
mod sizing;
mod state;

// Faz 9.8 — post-trade tick-driven modules.
pub mod allocator;
pub mod liquidation_guard;
pub mod live_position_store;
pub mod ratchet;
pub mod scale_manager;
pub mod selector;
pub mod tick_dispatcher;
pub mod tp_engine;

#[cfg(test)]
mod tests;

pub use checks::{
    DailyLossCheck, DrawdownCheck, KillSwitchCheck, LeverageCheck, MaxOpenPositionsCheck,
    RiskCheck, StopDistanceCheck,
};
pub use config::RiskConfig;
pub use engine::RiskEngine;
pub use error::{RiskError, RiskResult};
pub use sizing::{
    hint_tag, FixedNotionalSizer, KellySizer, RiskPctSizer, Sizer, SizerOutput, SizerRegistry,
    VolTargetSizer,
};
pub use state::AccountState;

// Faz 9.8 re-exports (skeleton — full logic across 9.8.3/9.8.5/9.8.6/9.8.7).
pub use allocator::{
    AllocationInput, AllocationOutcome, Allocator, AllocatorConfig, AllocatorGate,
    CommissionGate, CommissionRates, DrawdownGate, EquityFloorGate, MaxExposureGate,
};
pub use liquidation_guard::{
    action_db_tag as liquidation_action_db_tag, assess as assess_liquidation,
    assess_with_margin as assess_liquidation_with_margin, severity_db_tag as liquidation_severity_db_tag,
    LiquidationAction, LiquidationAssessment, LiquidationEventDto, LiquidationGuardConfig,
    LiquidationSeverity, MarginContext,
};
pub use live_position_store::{
    ExecutionMode, LivePositionState, LivePositionStore, MarketSegment, PositionId, PositionSide,
    TickKey, TpLeg,
};
pub use ratchet::{tighten_only, RatchetDecision, RatchetKind};
pub use scale_manager::{
    evaluate_with_context as evaluate_scale_with_context, unrealised_r, AddOnDipRule,
    PyramidInRule, ScaleContext, ScaleDecision, ScaleDecisionKind, ScaleManagerConfig, ScaleOutRule,
    ScaleRule, ScaleRuleRegistry,
};
pub use selector::{
    Direction as SelectorDirection, LiquidationCooldownFilter, MaxRiskPctFilter,
    MinAiScoreFilter, MinRiskRewardFilter, MinTierFilter, OpenPositionCapFilter, RankedOutcome,
    SegmentOverrides, SelectionOutcome, SelectorConfig, SelectorConfigOverride, SelectorFilter,
    SelectorRegistry, SetupCandidate,
};
pub use tick_dispatcher::{
    actionable_ids, evaluate_position as evaluate_position_tick, evaluate_tick,
    PositionTickOutcomes, TickContext, TickDispatcherConfig, TickOutcomes,
};
pub use tp_engine::TpTrigger;
