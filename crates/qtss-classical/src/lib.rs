//! qtss-classical — classical chart pattern detector.
//!
//! Pure detector that consumes a [`qtss_domain::v2::pivot::PivotTree`]
//! and reports classical chart formations on the most recent pivots:
//!
//! - Double Top / Double Bottom
//! - Head & Shoulders / Inverse Head & Shoulders
//! - Ascending / Descending / Symmetrical Triangle
//!
//! ## Design notes
//!
//! Each pattern lives as a [`ShapeSpec`] in `shapes.rs` — a `(name,
//! pivots_needed, eval_fn)` tuple. The detector walks every spec through
//! the same loop and keeps the best score; adding a new pattern (rising
//! wedge, flag, rectangle, …) is one entry in `SHAPES`, no central match
//! arm to edit (CLAUDE.md rule #1).
//!
//! All tolerances live in `ClassicalConfig` and are validated up-front
//! (CLAUDE.md rule #2). Bullish/bearish symmetry is encoded explicitly
//! per shape because chart patterns are not always price-symmetric the
//! way Elliott / harmonic XABCD structures are.
//!
//! Like every detector in QTSS v2, this crate emits only a structural
//! score; confidence and target prices are filled in later by the
//! validator and target-engine.

mod config;
mod detector;
mod error;
mod shapes;

#[cfg(test)]
mod tests;

pub use config::ClassicalConfig;
pub use detector::ClassicalDetector;
pub use error::{ClassicalError, ClassicalResult};
pub use shapes::{ShapeMatch, ShapeSpec, ShapeSpecBars, SHAPES, SHAPES_WITH_BARS};
