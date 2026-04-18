//! qtss-candles — Japanese candlestick pattern detector (Faz 10 Aşama 3).
//!
//! Asset-class agnostic (CLAUDE.md #4): consumes `Bar` slices only.
//! All thresholds live in [`CandleConfig`] and are seeded from
//! `system_config` via migration 0159 (CLAUDE.md #2). Each candle
//! pattern is a [`CandleSpec`] entry in [`CANDLE_SPECS`] — add a row,
//! no central `match` (CLAUDE.md #1).
//!
//! Supported patterns (subkind):
//!   Single-bar (N=1):
//!     * doji, dragonfly_doji, gravestone_doji, long_legged_doji
//!     * hammer (bull), inverted_hammer (bull)
//!     * hanging_man (bear), shooting_star (bear)
//!     * marubozu_bull, marubozu_bear
//!     * spinning_top
//!   Two-bar (N=2):
//!     * engulfing_bull, engulfing_bear
//!     * harami_bull, harami_bear
//!     * piercing_line (bull), dark_cloud_cover (bear)
//!     * tweezer_top (bear), tweezer_bottom (bull)
//!   Three-bar (N=3):
//!     * morning_star (bull), evening_star (bear)
//!     * three_white_soldiers (bull), three_black_crows (bear)
//!     * three_inside_up (bull), three_inside_down (bear)
//!     * three_outside_up (bull), three_outside_down (bear)

mod config;
mod detector;
mod error;
mod specs;

#[cfg(test)]
mod tests;

pub use config::CandleConfig;
pub use detector::CandleDetector;
pub use error::{CandleError, CandleResult};
pub use specs::{CandleMatch, CandleSpec, CANDLE_SPECS};
