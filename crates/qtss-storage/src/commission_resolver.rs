//! Commission lookup for the setup gate — one source of truth.
//!
//! Why its own module: the D/T/Q setup loop and the Wyckoff planner both
//! need a per-side commission rate for their "reject thin trades"
//! filters. Before this, each loop read a different key with different
//! defaults (D/T/Q: `setup.commission.taker_bps`=5, Wyckoff planner:
//! hardcoded 7.5 in `TradePlannerConfig::default()`). That drift
//! silently lets under-priced Wyckoff setups through while the D/T/Q
//! path blocks them — exactly the kind of CLAUDE.md #2 violation the
//! gap list calls out.
//!
//! Resolution order, per (venue_class, side):
//!   1. `setup.commission.{venue_class}.{side}_bps` — venue-specific
//!   2. `setup.commission.{side}_bps`               — global override
//!   3. the caller-supplied fallback                 — last-resort default
//!
//! All three paths land in `system_config` so GUI editors change them
//! without redeploying (CLAUDE.md #2).

use sqlx::PgPool;

use crate::resolve_system_f64;

/// Commission side for setup gating. Matches the bps key suffix so the
/// lookup is a formatting concern, not a match arm (CLAUDE.md #1).
#[derive(Debug, Clone, Copy)]
pub enum CommissionSide {
    Taker,
    Maker,
}

impl CommissionSide {
    fn key(self) -> &'static str {
        match self {
            CommissionSide::Taker => "taker_bps",
            CommissionSide::Maker => "maker_bps",
        }
    }
}

/// Resolve a single-side commission in basis points. See module docs
/// for the precedence ladder. `venue_class` is the same string written
/// into `qtss_setups.venue_class` (e.g. `binance_futures`).
pub async fn resolve_commission_bps(
    pool: &PgPool,
    venue_class: &str,
    side: CommissionSide,
    fallback_bps: f64,
) -> f64 {
    let side_key = side.key();

    // 1. Venue-specific.
    let venue_key = format!("commission.{venue_class}.{side_key}");
    let venue_val = resolve_system_f64(pool, "setup", &venue_key, "", f64::NAN).await;
    if venue_val.is_finite() && venue_val >= 0.0 {
        return venue_val;
    }

    // 2. Global.
    let global_key = format!("commission.{side_key}");
    let global_val = resolve_system_f64(pool, "setup", &global_key, "", f64::NAN).await;
    if global_val.is_finite() && global_val >= 0.0 {
        return global_val;
    }

    // 3. Caller fallback.
    fallback_bps
}
