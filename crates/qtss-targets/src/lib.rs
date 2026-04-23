//! qtss-targets — Take-profit / Stop-loss resolver layer.
//!
//! Strategy emits a `TradeIntent` with direction + entry; risk engine
//! needs TP/SL/invalidation to size and manage. This crate is the
//! bridge: given a pattern family + anchors + volatility context, it
//! returns a [`TargetSet`] with 1-3 TPs, a hard SL, and a soft
//! invalidation level.
//!
//! Dispatch is a table of [`TargetResolver`] trait impls keyed by
//! pattern family (CLAUDE.md #1 — adding a new resolver is one
//! entry, no central match). Every magic number lives in
//! [`TargetConfig`] seeded from `system_config` (CLAUDE.md #2).
//!
//! Resolver catalog:
//!
//!   * **HarmonicPrzResolver** — PRZ (Potential Reversal Zone) at D,
//!     projects TP1 = 0.382 × CD toward C, TP2 = 0.618 × CD,
//!     TP3 = 1.0 × CD. Stop: structural swing beyond D + buffer.
//!   * **VProfileMagnetResolver** — rides the nearest VPOC / VAH /
//!     VAL as magnetic targets. Uses pre-computed volume profile
//!     levels supplied via context.
//!   * **FibExtensionResolver** — swing X→A extensions (1.272 / 1.618
//!     / 2.618) for trend continuations; SL at X.
//!   * **StructuralInvalidationResolver** — SL at most-recent opposite
//!     pivot, TPs at next 2-3 same-direction pivots.
//!   * **AtrBandResolver** — fallback when no structural anchor set is
//!     available: TP1/2/3 = entry + k × ATR (1.5 / 3.0 / 5.0 default),
//!     SL = entry ∓ 1.0 × ATR.

mod config;
mod resolver;
mod resolvers;

pub use config::TargetConfig;
pub use resolver::{
    DetectionContext, ResolverRegistry, TargetLevel, TargetResolver, TargetSet, TargetSource,
};
pub use resolvers::{
    atr_band::AtrBandResolver, fib_extension::FibExtensionResolver,
    harmonic_prz::HarmonicPrzResolver, structural::StructuralInvalidationResolver,
    vprofile::VProfileMagnetResolver,
};
