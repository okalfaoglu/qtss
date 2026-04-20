//! Pivot engine configuration.
//!
//! **Faz 14.A15 — LuxAlgo birebir parite.** Previous iterations used an
//! ATR-threshold ZigZag (reversal >= `atr_mult * ATR`). We have switched
//! to a pure **pivot-window** detector, matching LuxAlgo's Elliott Waves
//! indicator 1:1:
//!   - A pivot High at bar `i` is any bar whose high is the maximum
//!     within the window `[i-length, i+length]`.
//!   - A pivot Low is the symmetric condition on lows.
//!   - ZigZag alternation is applied on top: consecutive same-kind
//!     candidates collapse to the most extreme one.
//!
//! Four window lengths correspond to four pivot levels. Lengths must be
//! strictly increasing so higher levels are guaranteed subsets of lower
//! levels (a length-8 pivot is necessarily also a length-4 pivot at the
//! same index — bigger window is a superset of the condition).
//!
//! The crate itself never touches the DB — the caller resolves the values
//! from `qtss-config` and constructs a `PivotConfig`.

use crate::error::{PivotError, PivotResult};

#[derive(Debug, Clone)]
pub struct PivotConfig {
    /// Pivot-window length per level (bars on each side). Index 0 = L0,
    /// index 3 = L3. Must be strictly increasing.
    ///
    /// Defaults `[4, 8, 16, 32]` mirror the LuxAlgo Elliott Waves
    /// indicator's common multi-scale configuration.
    pub lengths: [u32; 4],
}

impl PivotConfig {
    /// Defaults — LuxAlgo Elliott Waves parity.
    pub fn defaults() -> Self {
        Self {
            lengths: [4, 8, 16, 32],
        }
    }

    /// Validate the invariants the engine relies on.
    pub fn validate(&self) -> PivotResult<()> {
        for (i, l) in self.lengths.iter().enumerate() {
            if *l == 0 {
                return Err(PivotError::InvalidConfig(format!(
                    "lengths[{i}] must be >= 1"
                )));
            }
        }
        for i in 1..4 {
            if self.lengths[i] <= self.lengths[i - 1] {
                return Err(PivotError::InvalidConfig(format!(
                    "lengths must be strictly increasing (level {i})"
                )));
            }
        }
        Ok(())
    }
}
