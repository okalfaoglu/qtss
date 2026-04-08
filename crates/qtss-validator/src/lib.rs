//! qtss-validator — turns raw detector output into validated signals.
//!
//! Detectors stay pure: they only emit `structural_score`. The validator
//! is the layer that decides whether the structural geometry is *worth
//! trading on right now* by polling a set of confirmation channels and
//! blending their votes with the detector's own score into a final
//! `confidence` in `0..1`.
//!
//! ## Channels
//!
//! - **regime_alignment** — pattern family vs. current market regime
//!   (Elliott wants trending, Wyckoff/harmonic want ranging, …)
//! - **multi_timeframe** — agreement on a strictly higher timeframe
//! - **historical_hit_rate** — base rate from `qtss-reporting` stats
//!
//! Channels are dispatched through the [`ConfirmationChannel`] trait so
//! adding a new one is `impl ConfirmationChannel for X` plus a single
//! `validator.register(...)` call — no central match arm to edit
//! (CLAUDE.md rule #1).
//!
//! All weights, thresholds and the score floor live in
//! [`ValidatorConfig`] and are validated up-front (CLAUDE.md rule #2).

mod channels;
mod config;
mod context;
mod engine;
mod error;

#[cfg(test)]
mod tests;

pub use channels::{
    ConfirmationChannel, HistoricalHitRate, MultiTimeframeConfluence, RegimeAlignment,
};
pub use config::ValidatorConfig;
pub use context::{is_higher_timeframe, pattern_key, HitRateStat, ValidationContext};
pub use engine::Validator;
pub use error::{ValidatorError, ValidatorResult};
