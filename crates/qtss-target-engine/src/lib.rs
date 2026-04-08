//! qtss-target-engine — turns a `Detection` into a list of price targets.
//!
//! Detectors stay pure: they only emit anchors and a structural score.
//! The target engine is the layer that, given those anchors, computes
//! the actual price levels a strategy can place orders against.
//!
//! ## Methods
//!
//! Each projection method implements [`TargetMethodCalc`] and decides for
//! itself whether the candidate detection has the right shape to project
//! from. Methods that don't apply return an empty `Vec`. Adding a new
//! method (Wolfe-wave projection, ATR multiples, …) is one impl + one
//! `engine.register(...)` call — no central match arm to edit
//! (CLAUDE.md rule #1).
//!
//! Initial method set:
//! - `MeasuredMoveMethod`        — double top/bottom, head & shoulders
//! - `FibExtensionMethod`        — Elliott impulse 1.0 / 1.618 / 2.618
//! - `HarmonicRetracementMethod` — XABCD AD-leg 0.382 / 0.618 / 1.0
//! - `WyckoffRangeMethod`        — spring / upthrust 0.5x / 1.0x of range
//!
//! ## Clustering
//!
//! After every method has voted, the engine merges nearby targets into a
//! single weight-weighted cluster, then trims by weight floor and a hard
//! `max_targets` cap. Tolerances live in [`TargetEngineConfig`] and are
//! validated up-front (CLAUDE.md rule #2).

mod config;
mod engine;
mod error;
mod methods;

#[cfg(test)]
mod tests;

pub use config::TargetEngineConfig;
pub use engine::TargetEngine;
pub use error::{TargetEngineError, TargetEngineResult};
pub use methods::{
    direction_of, Direction, FibExtensionMethod, HarmonicRetracementMethod, MeasuredMoveMethod,
    TargetMethodCalc, WyckoffRangeMethod,
};
