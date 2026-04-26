//! IQ-D / IQ-T enterprise backtest pipeline (FAZ 26).
//!
//! Sits ALONGSIDE the existing v1 (`engine`) and v2 (`v2`) backtest
//! infrastructure. The v2 runner is generic — pluggable
//! `BarStream` / `SignalSource` / strategy provider — and is the
//! backbone for live-style replay. This `iq` module specialises the
//! v2 model for the user-defined Setup pipeline:
//!
//!   bar_stream (market_bars table)
//!       ↓
//!   detection_replay (re-detect Wyckoff/Elliott/IQ at each bar)
//!       ↓
//!   iq_setup_factory (IQ-D for Polarity::Dip / IQ-T for Top)
//!       ↓
//!   trade_lifecycle (entry, TP1/TP2/TP3 ladder, SL, timeout, trail)
//!       ↓
//!   cost_model (fee + slippage + funding)
//!       ↓
//!   attribution::classify_outcome
//!       ↓
//!   trade_log (JSONL per trade with full path)
//!
//! Goals (user spec):
//!   1. Enterprise-grade — handles years of historical data per
//!      (sym, tf), deterministic, parallelisable.
//!   2. Comprehensive logging — every loss carries a structured
//!      reason + component breakdown so post-mortem is mechanical.
//!   3. Optimisation hooks — weight grids, walk-forward, Bayesian
//!      (Bayesian lands in a follow-up commit; grid + walk-forward
//!      ship here).
//!   4. Live parity — the SAME scoring / gating code paths the
//!      worker runs, just over historical bars. No reimplemented
//!      heuristics.
//!
//! Entry point: [`IqBacktestRunner`] in `runner.rs`. Build via
//! [`config::IqBacktestConfig`] and feed a database pool +
//! symbol+tf+date-range. Returns a [`report::IqBacktestReport`]
//! plus emits per-trade JSONL if a log path is configured.

pub mod attribution;
pub mod cli;
pub mod config;
pub mod cost;
pub mod detector;
pub mod manager;
pub mod optimize;
pub mod report;
pub mod runner;
pub mod scorers;
pub mod trade;
pub mod trade_log;

pub use attribution::{LossReason, OutcomeAttribution, OutcomeClass};
pub use config::IqBacktestConfig;
pub use cost::{CostModel, FillCost};
pub use detector::IqReplayDetector;
pub use manager::IqLifecycleManager;
pub use optimize::{
    ConfigSummary, GridSpec, OptimizationReport, OptimizationResult,
    OptimizationRunner, SensitivityRow, WalkForwardSpec,
    WalkForwardWindow, WeightRange,
};
pub use report::IqBacktestReport;
pub use runner::IqBacktestRunner;
pub use trade::{IqTrade, TradeOutcome, TradeState};
pub use trade_log::TradeLogWriter;
