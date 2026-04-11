//! qtss-wyckoff — Full Wyckoff structure detector (Faz 10).
//!
//! Detects all Wyckoff phases (A–E) and events:
//! - Phase A: PS, SC/BC, AR, ST
//! - Phase B: UA, ST-B
//! - Phase C: Spring, UTAD, Shakeout
//! - Phase D: SOS, SOW, LPS, LPSY, JAC, Break of Ice, BUEC
//! - Phase E: Markup / Markdown
//!
//! Each event lives as an [`EventSpec`] in `events.rs`. Adding a new event
//! is one slice entry, no central match arm to edit (CLAUDE.md rule #1).
//!
//! The [`WyckoffStructureTracker`] maintains a state machine that tracks
//! phase progression and key levels (creek, ice, range).

mod config;
mod detector;
mod error;
mod events;
mod range;
pub mod structure;

#[cfg(test)]
mod tests;

pub use config::WyckoffConfig;
pub use detector::WyckoffDetector;
pub use error::{WyckoffError, WyckoffResult};
pub use events::{EventMatch, EventSpec, EVENTS};
pub use range::TradingRange;
pub use structure::{
    RecordedEvent, WyckoffEvent, WyckoffPhase, WyckoffSchematic,
    WyckoffStructureTracker,
};
