//! qtss-strategy v2 — `StrategyProvider` boundary.
//!
//! Strategies are pluggable: every concrete strategy (rule-based,
//! AI-driven, copy-trade, …) implements [`StrategyProvider`] and can
//! be hot-swapped from config without touching the worker. This file
//! defines the trait + a tiny rule strategy that exists primarily to
//! prove the contract round-trips through the rest of the v2 stack
//! (validator → strategy → risk → execution).
//!
//! ## Design (CLAUDE.md)
//!
//! - **No hardcoded thresholds (#2):** every numeric input lives in
//!   the per-strategy config struct; the worker fills it from
//!   `qtss_config` at boot.
//! - **No layer leakage (#3):** the trait sees `ValidatedDetection` in
//!   and emits `TradeIntent` out — nothing about brokers, sizing
//!   models, fees or order types. Risk/execution take it from there.
//! - **Strategy dispatch is a registry (#1):** a worker keeps
//!   `HashMap<String, Arc<dyn StrategyProvider>>` so adding a strategy
//!   is one `register` call, never a central match.

mod error;
mod provider;
mod rule;

pub use error::{StrategyError, StrategyResult};
pub use provider::{StrategyContext, StrategyProvider};
pub use rule::{ConfidenceThresholdStrategy, ConfidenceThresholdStrategyConfig};
