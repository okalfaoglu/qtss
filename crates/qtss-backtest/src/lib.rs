//! Backtest ve parametre optimizasyonu: **timestamp bar** ile çalışır; tick ile genişletmeye hazır.
//!
//! - `Strategy`: her barda sinyal / pozisyon güncellemesi
//! - `BacktestEngine`: slippage, komisyon, pozisyon limitleri (genişletilebilir)
//! - `WalkForward`: pencere kaydırarak robustness
//! - `optimize::grid_search`: parametre ızgarası + walk-forward skoru

pub mod v2;
pub use v2::{
    BacktestRunner, BacktestSummary, BacktestV2Config, BarStream, SignalSource,
};

pub mod engine;
pub mod metrics;
pub mod optimize;
pub mod strategy;

pub use engine::{BacktestConfig, BacktestEngine, BacktestResult, EquityPoint};
pub use metrics::PerformanceReport;
pub use optimize::{OptimizationResult, Optimizer, ParameterGrid, WalkForwardConfig};
pub use strategy::Strategy;

// FAZ 26 — IQ-D / IQ-T enterprise backtest pipeline. Lives alongside
// v1 / v2 — re-uses bar-stream + cost ideas but focuses on the
// user-defined Setup pipeline (Wyckoff + Elliott + cycle confluence)
// and adds first-class loss attribution.
pub mod iq;
