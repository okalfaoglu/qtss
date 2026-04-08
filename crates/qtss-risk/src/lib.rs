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
