//! Pure technical indicators for TBM (Top/Bottom Mining).
#![allow(unused)]

pub mod ema;
pub mod macd;
pub mod bollinger;
pub mod stochastic;
pub mod mfi;
pub mod obv;
pub mod cvd;
pub mod vwap;
pub mod fibonacci;
pub mod volatility;
pub mod divergence;
pub mod indicator_bundle;
pub mod volume_profile;

// Faz 11 Aşama 5 — Indicator depth expansion (CLAUDE.md #1: one module
// per indicator, no central match; CLAUDE.md #2: every period/factor
// configurable via system_config at the call-site). Each module is a
// pure function or a small result struct — stateless, no shared state,
// ready for parallel compute in the API layer.
pub mod rsi;
pub mod supertrend;
pub mod ichimoku;
pub mod donchian;
pub mod keltner;
pub mod williams_r;
pub mod cmf;
pub mod aroon;
pub mod ad_line;
pub mod psar;
pub mod chandelier;
pub mod ttm_squeeze;

pub use ema::{ema, ema_step, sma};
pub use macd::{macd, MacdResult};
pub use bollinger::{bollinger, BollingerResult};
pub use stochastic::{stochastic, StochasticResult};
pub use mfi::mfi;
pub use obv::obv;
pub use cvd::cvd;
pub use vwap::{vwap, VwapResult};
pub use fibonacci::{fib_retracements, fib_extensions, FibLevel};
pub use volatility::{atr, bb_squeeze, compression_detector};
pub use divergence::{detect_divergences, Divergence, DivergenceType};
pub use indicator_bundle::{compute_all, IndicatorBundle};
pub use volume_profile::{volume_profile, OhlcvInput, VolumeProfile};

pub use rsi::rsi;
pub use supertrend::{supertrend, SuperTrendResult};
pub use ichimoku::{ichimoku, Ichimoku};
pub use donchian::{donchian, Donchian};
pub use keltner::{keltner, Keltner};
pub use williams_r::williams_r;
pub use cmf::cmf;
pub use aroon::{aroon, AroonResult};
pub use ad_line::ad_line;
pub use psar::{psar, PsarResult};
pub use chandelier::{chandelier, Chandelier};
pub use ttm_squeeze::ttm_squeeze;
