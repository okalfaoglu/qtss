//! qtss-harmonic — XABCD harmonic pattern detector.
//!
//! Pure detector that consumes a [`qtss_domain::v2::pivot::PivotTree`]
//! produced by `qtss-pivots` and reports classical harmonic patterns
//! (Gartley, Bat, Butterfly, Crab) on the latest five pivots.
//!
//! ## Design notes
//!
//! Every pattern is encoded as a [`HarmonicSpec`] in `patterns.rs` — a
//! small struct of (label, four ratio ranges). The matcher walks the
//! same loop for every pattern; adding a new harmonic = appending one
//! `HarmonicSpec` to the table, no central match arm to edit
//! (CLAUDE.md rule #1).
//!
//! Both bullish and bearish forms are detected by the same code: the
//! detector negates prices for the bearish branch and reuses the
//! identical ratio checks.
//!
//! Like every detector in QTSS v2, this crate emits only a structural
//! score. Confidence and target prices are filled in later by the
//! validator and target-engine respectively.

mod config;
mod detector;
mod error;
mod matcher;
mod patterns;

#[cfg(test)]
mod tests;

pub use config::HarmonicConfig;
pub use detector::HarmonicDetector;
pub use error::{HarmonicError, HarmonicResult};
pub use matcher::{match_pattern, RatioRange, XabcdPoints};
pub use patterns::{HarmonicSpec, PATTERNS};
