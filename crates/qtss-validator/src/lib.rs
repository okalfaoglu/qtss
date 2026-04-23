//! qtss-validator — invalidates stale detections when price action
//! breaks their geometry. Worker `validator_loop` calls
//! `ValidatorRegistry::validate` on each tick for every non-invalidated
//! row; anything flagged `invalidated = true` stops showing up on the
//! chart and can't be acted on by the strategy layer.
//!
//! Dispatch is a `HashMap<family, Arc<dyn Validator>>` so adding a new
//! family's validator is a one-line registration, no central match
//! (CLAUDE.md #1). Every threshold lives in `system_config.validator.*`
//! (CLAUDE.md #2).
//!
//! Family invalidation rules this release:
//!   * **harmonic**   — price closes beyond PRZ by > `harmonic_break_pct`
//!   * **classical**  — close beyond invalidation_price in `raw_meta`
//!   * **range**      — zone fill_pct reaches `range_full_fill_pct`
//!   * **gap**        — gap close reaches `gap_close_pct` of original gap
//!   * **motive**     — close below wave-1 low (bull) / above wave-1 high (bear)
//!   * **smc**        — bar closes beyond event's `invalidation_price`
//!   * **orb**        — close back inside OR after break
//!
//! Generic `Validator::validate` sees a snapshot { detection row + current
//! price + ATR }; returns `ValidatorVerdict::{ Hold, Invalidate }`.

mod config;
mod registry;
mod validators;
mod verdict;

pub use config::ValidatorConfig;
pub use registry::{default_registry, DetectionRow, ValidatorRegistry};
pub use validators::Validator;
pub use verdict::{InvalidationReason, ValidatorVerdict};
