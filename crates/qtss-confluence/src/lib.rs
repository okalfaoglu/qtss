//! qtss-confluence — Faz 7.8 dual-track scoring.
//!
//! Two independent numbers per (symbol, timeframe), produced from
//! whatever fresh evidence the worker can collect inside a short
//! window:
//!
//! 1. **erken_uyari** (early warning) — TBM raw direction in `[-1, +1]`.
//!    Pass-through of the Top/Bottom Mining score so the GUI can light
//!    up the moment structure flips, even before any pattern detector
//!    has confirmed.
//!
//! 2. **guven** (confidence) — `[0, 1]`, weighted agreement across
//!    independent layers (Elliott / Harmonic / Classical / Wyckoff /
//!    Range / TBM / Onchain). The Setup Engine (Faz 8.0) will not arm
//!    a setup until `guven >= threshold`.
//!
//! ## Why two scores
//!
//! A single blended number hides whether the call is "fast but lonely"
//! (only TBM saw it) or "slow and crowded" (multiple structural
//! detectors agreed). The Q-RADAR state machine (ZAYIF DİP / MUHTEMEL
//! TEPE / GÜÇLÜ DİP) needs both axes to choose a label.
//!
//! ## Hard rule: `min_layers`
//!
//! Below the configured layer threshold (default 3) `guven` is **0**.
//! `direction` is preserved so post-mortem queries can answer "TBM
//! said long but only one detector backed it — why didn't we trade?".
//!
//! Pure crate: no DB, no HTTP, no asset-class assumptions
//! (CLAUDE.md #4). The worker loop loads inputs, this crate scores.

pub mod scoring;
pub mod types;

pub use scoring::score_confluence;
pub use types::{
    ConfluenceDirection, ConfluenceInputs, ConfluenceReading, ConfluenceWeights, DetectionVote,
};
