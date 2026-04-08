//! qtss-wyckoff — Wyckoff trading-range and event detector.
//!
//! Pure detector that consumes a [`qtss_domain::v2::pivot::PivotTree`]
//! and reports Wyckoff structures on the most recent pivots:
//!
//! - Trading range (accumulation / distribution / neutral, classified
//!   by the climactic-volume pivot's location)
//! - Spring (bullish false-break of support)
//! - Upthrust / UTAD (bearish false-break of resistance)
//!
//! ## Design notes
//!
//! Each event lives as an [`EventSpec`] in `events.rs` — a `(name,
//! eval_fn)` pair. The detector walks every spec through the same loop
//! and keeps the highest-scoring match; adding a new event (SOS, LPS,
//! Sign-of-Weakness, …) is one slice entry, no central match arm to
//! edit (CLAUDE.md rule #1).
//!
//! All thresholds (range tightness, climax volume multiplier,
//! penetration band, score floor) live in [`WyckoffConfig`] and are
//! validated up-front (CLAUDE.md rule #2).
//!
//! Like every detector in QTSS v2, this crate emits only a structural
//! score; confidence and target prices are filled in later by the
//! validator and target-engine.

mod config;
mod detector;
mod error;
mod events;
mod range;

#[cfg(test)]
mod tests;

pub use config::WyckoffConfig;
pub use detector::WyckoffDetector;
pub use error::{WyckoffError, WyckoffResult};
pub use events::{EventMatch, EventSpec, EVENTS};
pub use range::TradingRange;
