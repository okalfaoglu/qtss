//! qtss-wyckoff — Wyckoff accumulation / distribution phase + event
//! detector (Faz 14).
//!
//! Twelve events across accumulation + distribution schematics. Each
//! event is a [`WyckoffSpec`] entry in [`WYCKOFF_SPECS`] — detector
//! walks every spec through the same loop (CLAUDE.md #1).
//!
//! Accumulation events: PS / SC / AR / ST / Spring / Test / SOS / LPS /
//! BU.
//! Distribution events: BC / UTAD / SOW.
//!
//! Event codes:
//!   PS   — Preliminary Support: first stop in downtrend
//!   SC   — Selling Climax: high-volume panic low
//!   AR   — Automatic Rally: bounce after SC
//!   ST   — Secondary Test: low-volume retest of SC
//!   Sp   — Spring: shakeout below range with reclaim
//!   Test — Test of spring
//!   SOS  — Sign of Strength: bullish impulse in range
//!   LPS  — Last Point of Support: higher low after SOS
//!   BU   — Back-Up / Jump-across-creek: breakout + pullback
//!   BC   — Buying Climax: distribution mirror of SC
//!   UTAD — Upthrust After Distribution: shakeout above range
//!   SOW  — Sign of Weakness: bearish impulse in range
//!
//! Phase A-E state machine is tracked per range via
//! [`WyckoffPhaseTracker`] — consumers (the engine writer) hand it
//! events in chronological order and read `phase()` / `bias()`.

mod config;
mod cycle;
mod event;
mod events;
mod phase;
mod range;

pub use config::WyckoffConfig;
pub use cycle::{detect_cycles, WyckoffCycle, WyckoffCyclePhase};
pub use event::{WyckoffEvent, WyckoffEventKind, WyckoffSpec};
pub use events::{detect_events, WYCKOFF_SPECS};
pub use phase::{WyckoffBias, WyckoffPhase, WyckoffPhaseTracker};
pub use range::{detect_ranges, WyckoffRange};
