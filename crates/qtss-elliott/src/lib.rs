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

mod aggregator;
mod combination;
mod common;
mod decomposition;
mod config;
mod detector;
mod diagonal;
mod error;
mod extended_impulse;
mod fibs;
mod flat;
mod formation;
mod forming;
mod nascent;
mod projection;
pub mod projection_engine;
pub mod rules;
mod triangle;
mod truncated_fifth;
mod zigzag;

#[cfg(test)]
mod tests;

pub use aggregator::{ElliottDetectorSet, ElliottFormationToggles};
pub use combination::CombinationDetector;
pub use config::ElliottConfig;
pub use detector::ImpulseDetector;
pub use diagonal::{DiagonalDetector, DiagonalKind};
pub use error::{ElliottError, ElliottResult};
pub use extended_impulse::ExtendedImpulseDetector;
pub use flat::FlatDetector;
pub use formation::FormationDetector;
pub use forming::FormingImpulseDetector;
pub use nascent::NascentImpulseDetector;
pub use triangle::TriangleDetector;
pub use truncated_fifth::TruncatedFifthDetector;
pub use zigzag::ZigzagDetector;

// Faz 12.C — re-exports for `elliott-backtest-sweep`. The binary mimics
// `ImpulseDetector::detect_all` inline (to avoid building a full
// `PivotTree` per level just to discard 3 of its 4 slots), but must use
// the crate's authoritative rule table + scorer so backtest never
// drifts from live semantics.
pub use rules::{ImpulsePoints, Rule, RULES};
pub use detector::score_impulse;
