//! PublicCard — user-facing, channel-agnostic view of a setup.
//!
//! Submodules:
//!   * [`tier`]     — AI score → N/10 tier + progress bar
//!   * [`category`] — (exchange, symbol) → asset category
//!   * [`builder`]  — SetupSnapshot → PublicCard
//!   * [`config`]   — DB-backed resolvers for tier & category thresholds

pub mod builder;
pub mod category;
pub mod config;
pub mod tier;

pub use builder::{PublicCard, SetupDirection, SetupSnapshot, TargetPoint};
pub use category::{AssetCategory, CategoryThresholds, ResolveContext};
pub use tier::{ScoreTier, TierBadge, TierThresholds};
