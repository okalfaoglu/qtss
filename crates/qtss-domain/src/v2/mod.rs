//! qtss-domain v2 — asset-class agnostic core types.
//!
//! Introduced for the v2 architecture (see `docs/QTSS_V2_ARCHITECTURE_PLAN.md`).
//! Lives alongside the legacy crypto-centric types in the parent module so the
//! existing crates keep building during the migration.
//!
//! ## Layering
//! - **instrument** — Venue, AssetClass, SessionCalendar, Instrument
//! - **timeframe**  — Timeframe enum, conversion helpers
//! - **bar**        — venue/asset-class agnostic OHLCV bar (`v2::Bar`)
//! - **pivot**      — multi-level pivot tree (L0..L3) shared by all detectors
//! - **regime**     — market regime snapshot
//! - **detection**  — pattern detection contract (Detection, Target, ...)
//! - **intent**     — TradeIntent, OrderRequest, Side, OrderType, RunMode
//!
//! ## Design rules (CLAUDE.md)
//! - Types only — no DB, no behavior beyond constructors and pure helpers.
//! - Asset-class agnostic: a crate that depends on `v2::Bar` works for
//!   crypto, BIST, and NASDAQ without code changes.
//! - Serde + Hash + Eq where it makes sense for event bus / DB JSON.

pub mod bar;
pub mod detection;
pub mod instrument;
pub mod intent;
pub mod pivot;
pub mod regime;
pub mod timeframe;

pub use bar::Bar;
pub use detection::{Detection, PatternKind, PatternState, PivotRef, Target, TargetMethod};
pub use instrument::{AssetClass, Instrument, SessionCalendar, Venue};
pub use intent::{
    OrderRequest, OrderType, RunMode, Side, SizingHint, TimeInForce as V2TimeInForce, TradeIntent,
};
pub use pivot::{Pivot, PivotKind, PivotLevel, PivotTree};
pub use regime::{RegimeKind, RegimeSnapshot, TrendStrength};
pub use timeframe::Timeframe;
