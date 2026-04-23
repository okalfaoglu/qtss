//! qtss-regime — incremental market regime detector.
//!
//! Consumes [`qtss_domain::v2::bar::Bar`] and emits a
//! [`qtss_domain::v2::regime::RegimeSnapshot`] classifying the current
//! state as TrendingUp/Down, Ranging, Squeeze, Volatile, or Uncertain
//! using ADX, Bollinger band width, ATR%, and the Choppiness Index.
//!
//! ## Why these indicators
//!
//! * **ADX (+DI / -DI)** — trend strength and direction.
//! * **Bollinger Band width** — squeeze (low volatility coiling) detection.
//! * **ATR / price** — absolute volatility scaled to instrument price.
//! * **Choppiness Index** — distinguishes range from trend independent of ADX.
//!
//! ## Classification
//!
//! Done via an ordered rule table (`classifier.rs`). Each rule is a closure
//! that inspects the four indicators; the first matching rule wins. Adding
//! a new regime kind = appending one rule. No scattered `if/else` in the
//! engine — see CLAUDE.md rule #1.
//!
//! ## Purity
//!
//! No DB, no IO, no global state. Thresholds come in via `RegimeConfig`;
//! production wiring resolves them from `qtss-config`.

mod adx;
mod bbands;
mod choppiness;
mod classifier;
mod config;
mod engine;
mod error;
pub mod multi_tf;
pub mod session;
pub mod transition;

#[cfg(test)]
mod tests;

pub use adx::AdxState;
pub use bbands::BBandsState;
pub use choppiness::ChoppinessState;
pub use classifier::classify;
pub use config::RegimeConfig;
pub use engine::RegimeEngine;
pub use error::{RegimeError, RegimeResult};
