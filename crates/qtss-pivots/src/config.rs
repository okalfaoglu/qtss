//! Pivot engine configuration.
//!
//! All numbers in here ultimately come from `qtss-config` (see migration
//! 0016 for the seeded keys: `pivots.zigzag.atr_period`, `atr_mult_l0..l3`).
//! The crate itself never touches the DB — the caller resolves the values
//! and constructs a `PivotConfig`. This keeps the crate pure and trivial
//! to unit-test with arbitrary thresholds.

use crate::error::{PivotError, PivotResult};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Debug, Clone)]
pub struct PivotConfig {
    pub atr_period: usize,
    /// Reversal multipliers per level. Index 0 = L0, index 3 = L3.
    /// Each multiplier MUST be strictly greater than the previous one
    /// so higher levels are guaranteed to be subsets of lower levels.
    pub atr_mult: [Decimal; 4],
}

impl PivotConfig {
    /// Defaults that mirror migration 0016. Used by tests so they don't
    /// need to wire qtss-config in.
    pub fn defaults() -> Self {
        Self {
            atr_period: 14,
            atr_mult: [dec!(1.5), dec!(3.0), dec!(6.0), dec!(12.0)],
        }
    }

    /// Validate the invariants the engine relies on. Called by
    /// `PivotEngine::new` so misconfiguration fails loud at startup
    /// instead of silently producing degenerate trees.
    pub fn validate(&self) -> PivotResult<()> {
        if self.atr_period < 2 {
            return Err(PivotError::InvalidConfig(
                "atr_period must be >= 2".into(),
            ));
        }
        for (i, m) in self.atr_mult.iter().enumerate() {
            if *m <= dec!(0) {
                return Err(PivotError::InvalidConfig(format!(
                    "atr_mult[{i}] must be positive"
                )));
            }
        }
        for i in 1..4 {
            if self.atr_mult[i] <= self.atr_mult[i - 1] {
                return Err(PivotError::InvalidConfig(format!(
                    "atr_mult must be strictly increasing (level {i})"
                )));
            }
        }
        Ok(())
    }
}
