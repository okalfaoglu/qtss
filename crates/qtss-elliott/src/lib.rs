//! qtss-elliott — Elliott wave detector.
//!
//! Pure detector that consumes a [`qtss_domain::v2::pivot::PivotTree`]
//! produced by `qtss-pivots` and reports impulse 1-2-3-4-5 structures
//! (both bullish and bearish) as [`qtss_domain::v2::detection::Detection`]
//! envelopes.
//!
//! ## What this PR covers
//!
//! * Bullish & bearish 5-wave **impulse** detection on a chosen
//!   `PivotLevel`.
//! * Standard validity rules:
//!     1. Wave 2 may not retrace beyond the start of wave 1.
//!     2. Wave 3 may not be the shortest of waves 1, 3, 5.
//!     3. Wave 4 may not enter the price territory of wave 1
//!        (no overlap, ignoring diagonals for now).
//! * Fibonacci proximity scoring for the structural score:
//!     * Wave 2 retracement vs {0.382, 0.5, 0.618}
//!     * Wave 3 extension vs {1.618, 2.618}
//!     * Wave 4 retracement vs {0.236, 0.382}
//!
//! ## What is **not** in this PR
//!
//! * Corrective ABC, diagonals, complex W-X-Y combinations.
//! * Confidence and target prices — those are filled in by the validator
//!   and target-engine respectively. The detector contract in
//!   `qtss-domain` makes this enforced at the type level.

mod config;
mod detector;
mod error;
mod fibs;
mod rules;

#[cfg(test)]
mod tests;

pub use config::ElliottConfig;
pub use detector::ImpulseDetector;
pub use error::{ElliottError, ElliottResult};
