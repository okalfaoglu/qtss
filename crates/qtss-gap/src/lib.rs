//! qtss-gap — gap & island reversal detector (Faz 10 Aşama 2).
//!
//! Asset-class agnostic (CLAUDE.md #4): consumes only `Bar` slices.
//! All thresholds live in [`GapConfig`] and are seeded from
//! `system_config` (CLAUDE.md #2). Each gap kind is a [`GapSpec`] entry
//! in [`GAP_SPECS`] — adding a variant is a row, no central match
//! (CLAUDE.md #1).
//!
//! Detected subkinds:
//!   * `common_gap_{bull|bear}`
//!   * `breakaway_gap_{bull|bear}`
//!   * `runaway_gap_{bull|bear}`
//!   * `exhaustion_gap_{bull|bear}`
//!   * `island_reversal_{bull|bear}`
//!
//! Gap classification rules:
//!   * Gap magnitude: `|open_t - close_{t-1}| / close_{t-1} >= min_gap_pct`
//!   * Breakaway: gap from tight consolidation range (range < `range_flat_pct`) in
//!     direction of nascent trend, with volume ≥ `vol_mult_breakaway` × SMA(vol)
//!   * Runaway: gap in the direction of an already-established trend
//!     (>= `runaway_trend_bars` same-sign bars with cumulative return ≥
//!     `runaway_trend_min_pct`), volume ≥ `vol_mult_runaway` × SMA(vol)
//!   * Exhaustion: gap in trend direction but at/near extreme; followed within
//!     `exhaustion_reversal_bars` by a reversal (close < pre-gap close for bull,
//!     vice versa). Volume ≥ `vol_mult_exhaustion` × SMA(vol)
//!   * Island reversal: gap in one direction, plateau of `island_max_bars` bars,
//!     then gap in opposite direction with both gaps `>= min_gap_pct`
//!   * Common: fallback when no other spec matches

mod config;
mod detector;
mod error;
mod specs;

#[cfg(test)]
mod tests;

pub use config::GapConfig;
pub use detector::GapDetector;
pub use error::{GapError, GapResult};
pub use specs::{GapMatch, GapSpec, GAP_SPECS};
