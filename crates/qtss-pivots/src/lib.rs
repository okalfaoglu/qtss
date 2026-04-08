//! qtss-pivots — central multi-level zigzag / pivot engine.
//!
//! See `docs/QTSS_V2_ARCHITECTURE_PLAN.md` §4 for the design rationale.
//!
//! # What this crate does
//!
//! Consume a stream of [`qtss_domain::v2::bar::Bar`] values and produce a
//! [`qtss_domain::v2::pivot::PivotTree`] with four nested levels (L0..L3).
//! Each level uses progressively coarser ATR-based reversal thresholds so
//! the same algorithm yields micro swings, intermediate swings, swing
//! highs/lows, and macro pivots from a single bar series.
//!
//! # Why one crate
//!
//! Every pattern detector (Elliott, harmonic, classical, Wyckoff, range)
//! reads from this same tree. Centralizing the pivot logic here means a
//! detector never has to roll its own zigzag and the subset invariant
//! between levels is enforced once for all consumers.
//!
//! # Purity
//!
//! The crate is pure: no DB, no network, no global state. Configuration is
//! passed by the caller via [`PivotConfig`]; production wiring resolves
//! those numbers from `qtss-config` and constructs the engine. Tests can
//! pass any config they like without touching infrastructure.

mod atr;
mod config;
mod engine;
mod error;
mod zigzag;

#[cfg(test)]
mod tests;

pub use atr::AtrState;
pub use config::PivotConfig;
pub use engine::{NewPivot, PivotEngine};
pub use error::{PivotError, PivotResult};
pub use zigzag::{Sample, ZigZag};
