//! qtss-smc — Smart Money Concepts detector.
//!
//! Event-oriented sibling of `qtss-range` (which tracks zones). Where
//! `qtss-range` answers "is this bar inside a known supply/demand
//! zone?", qtss-smc answers "did a structural shift just happen?".
//! Five event families:
//!
//!   * **BOS** (Break of Structure) — continuation: a new HH above the
//!     most recent HH in an existing uptrend (or LL below LL in a
//!     downtrend). Confirms the trend.
//!   * **CHoCH** (Change of Character) — reversal: first LH after a
//!     sequence of HHs (bearish CHoCH) or first HL after LLs (bullish).
//!     Structural trend flip.
//!   * **MSS** (Market Structure Shift) — stricter CHoCH: the break
//!     must clear a prior swing's close, not just its extreme.
//!   * **Liquidity Sweep** — price wicks through a prior swing high/low
//!     with a strong rejection (stop hunt). The classic "smart money
//!     fill" signal.
//!   * **FVI** (Fair Value Imbalance) — 3-candle imbalance like FVG but
//!     stricter: volume must be above average on the middle candle.
//!     The tighter cousin of qtss-range's FVG.
//!
//! Asset-class agnostic (CLAUDE.md #4) — consumes `PivotTree` + `Bar`
//! slices only. Each detector is a [`SmcSpec`] entry in [`SMC_SPECS`]
//! — adding a variant is one row, no central match (CLAUDE.md #1).
//! All thresholds read from `SmcConfig` at call-site, seeded from
//! `system_config` (CLAUDE.md #2).

mod config;
mod detector;
mod event;
mod events;

pub use config::SmcConfig;
pub use detector::{SmcDetector, SMC_SPECS};
pub use event::{SmcEvent, SmcEventKind, SmcSpec};
