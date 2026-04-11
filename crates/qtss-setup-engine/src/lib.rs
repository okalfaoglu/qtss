//! qtss-setup-engine — Faz 8.0 Setup Engine foundation.
//!
//! This crate owns the lifecycle of a *setup*: the bridge between a
//! `ConfluenceReading` (Faz 7.8) and a TradeIntent that Risk/Execution
//! will eventually act on. The engine itself is profile-aware
//! (`T` short-term, `Q` mid-term, `D` long-term) and carries three
//! collaborating pieces:
//!
//! 1. **PositionGuard** — per-setup ratchet trailing stop (`koruma`).
//!    Tightens monotonically as unrealised R climbs. Single source of
//!    truth for entry, entry_sl and active stop.
//!
//! 2. **Allocator** — cross-setup risk cap, max-concurrent-per-profile
//!    and correlation group cap. Decides whether a new candidate
//!    setup can be armed given the current open book.
//!
//! 3. **Reverse evaluator** — closes an active setup early when the
//!    latest confluence reading flips direction with sufficient
//!    `guven` (per-profile threshold).
//!
//! Foundation note: this file only defines the types and signatures.
//! The actual math and DB glue land in step 2 of Faz 8.0. All
//! public functions currently return `todo!()` so `cargo check`
//! stays green while the rest of the phase is wired up.
//!
//! CLAUDE.md compliance:
//!  - No hardcoded thresholds — every knob arrives via `*Config`
//!    structs populated from `system_config` by the worker loop.
//!  - No asset-class assumptions — the crate only sees prices,
//!    directions and profiles; venue-specific behaviour stays in
//!    adapters.

pub mod allocator;
pub mod classifier;
pub mod guard;
pub mod reverse;
pub mod sharing;
pub mod types;

pub use allocator::{
    check_allocation, AllocatorContext, AllocatorLimits, OpenSetupSummary,
};
pub use classifier::classify_alt_type;
pub use guard::{PositionGuard, PositionGuardConfig};
pub use reverse::should_reverse_close;
pub use sharing::{evaluate_sharing, QRadarShareInfo, SharingChannel, SharingConfig, SharingDecision};
pub use types::{
    AltType, CloseReason, Direction, Profile, RejectReason, RiskMode, RiskModeBehavior,
    SetupState, VenueClass,
};
