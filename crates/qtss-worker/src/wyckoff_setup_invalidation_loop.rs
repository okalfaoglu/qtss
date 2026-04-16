//! Wyckoff Setup Invalidation Sweeper — Faz 10 / P7.6.
//!
//! Watches every open (`armed` | `active`) Wyckoff setup and closes it
//! the moment price breaches its stop. Two distinct breach classes,
//! distinguishable so the UI / audit log can tell "tactical" from
//! "structural" failure:
//!
//!   * **`sl_breach`** — the tight SL (`entry_sl`) was taken out. The
//!     setup's tactical invalidation has triggered; a retry may still
//!     be valid if the wider structural SL holds.
//!   * **`structural_invalidated`** — the structural SL (P7.3 `sl_wide`
//!     in `wyckoff_classic_json`) was breached. The whole Wyckoff
//!     range hypothesis is dead; reverse-bias has now landed.
//!
//! Why a separate loop instead of the main setup loop:
//!   * The emitter loop only runs when a structure is active and a
//!     *new* setup is forming — it has no business tracking every
//!     prior open setup.
//!   * A short, tight tick (≤60 s) keeps invalidations visible on the
//!     Detections panel without dragging the detection pipeline.
//!
//! CLAUDE.md compliance:
//!   * #1: trait-free but every branch is a one-line match → a helper
//!     fn. No nested conditionals.
//!   * #2: tick, enabled, alt-type filter — all `system_config`.
//!   * #5: runtime mode awareness is implicit — we act on *every* open
//!     Wyckoff setup regardless of mode; `mode` only gates *new* setup
//!     emission in the main loop.

use std::time::Duration;

use rust_decimal::prelude::ToPrimitive;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use tracing::{debug, info, warn};

use qtss_storage::{
    list_recent_bars, list_v2_setups_filtered, resolve_system_f64, resolve_system_u64,
    resolve_worker_enabled_flag, update_v2_setup_state, SetupFilter, V2SetupRow,
};

// =========================================================================
// Entry point
// =========================================================================

pub async fn wyckoff_setup_invalidation_loop(pool: PgPool) {
    const DEFAULT_TICK_SECS: u64 = 60;

    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "setup",
            "wyckoff.invalidation.enabled",
            "",
            true,
        )
        .await;
        let tick = resolve_system_u64(
            &pool,
            "setup",
            "wyckoff.invalidation.interval_seconds",
            "",
            DEFAULT_TICK_SECS,
            10,
            600,
        )
        .await;

        if !enabled {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        }

        match run_pass(&pool).await {
            Ok(n) if n > 0 => info!(closed = n, "wyckoff_invalidation: pass closed setups"),
            Ok(_) => {}
            Err(e) => warn!(%e, "wyckoff_invalidation: pass failed"),
        }

        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}

// =========================================================================
// One pass
// =========================================================================

async fn run_pass(pool: &PgPool) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    // Limit is generous — there should only be tens of open setups
    // per universe at any given time.
    let filter = SetupFilter {
        limit: 500,
        alt_type_like: Some("wyckoff_%".to_string()),
        ..SetupFilter::default()
    };
    let setups = list_v2_setups_filtered(pool, &filter).await?;
    if setups.is_empty() {
        return Ok(0);
    }

    // Configurable slack around the stop to avoid single-tick noise
    // flickering the state. Expressed in *price ratio* units (e.g.
    // 0.0005 = 5 bps). Default 0 — breach = hard breach.
    let slack_ratio = resolve_system_f64(
        pool,
        "setup",
        "wyckoff.invalidation.breach_slack_ratio",
        "",
        0.0,
    )
    .await;

    let mut closed = 0usize;
    for row in setups {
        match maybe_invalidate(pool, &row, slack_ratio).await {
            Ok(true) => closed += 1,
            Ok(false) => {}
            Err(e) => warn!(setup_id=%row.id, %e, "wyckoff_invalidation: setup failed"),
        }
    }
    Ok(closed)
}

// =========================================================================
// Per-setup decision
// =========================================================================

async fn maybe_invalidate(
    pool: &PgPool,
    row: &V2SetupRow,
    slack_ratio: f64,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    // Without an entry SL we cannot decide. Skip — lifecycle_manager
    // will eventually time-close these.
    let entry_sl = match row.entry_sl {
        Some(x) => x as f64,
        None => return Ok(false),
    };
    let dir = match row.direction.as_str() {
        "long" | "LONG" | "Long" => Direction::Long,
        "short" | "SHORT" | "Short" => Direction::Short,
        _ => return Ok(false),
    };

    let (exchange, segment) = venue_to_pair(&row.venue_class);
    // Latest closed bar. 3 is enough — we only look at `[0]`.
    let bars = list_recent_bars(pool, exchange, segment, &row.symbol, &row.timeframe, 1).await?;
    let last = match bars.first() {
        Some(b) => b,
        None => return Ok(false),
    };
    let last_close = last.close.to_f64().unwrap_or(0.0);
    if !(last_close > 0.0) {
        return Ok(false);
    }

    // Structural SL is optional — only present on setups emitted after
    // P7.3. Treat absence as "no wider net" and rely on tight SL only.
    let sl_wide = extract_sl_wide(&row.raw_meta);

    // Decide which (if any) breach fired. Structural trumps tight.
    let decision = classify_breach(dir, last_close, entry_sl, sl_wide, slack_ratio);
    let Some((reason, breach_price)) = decision else {
        return Ok(false);
    };

    update_v2_setup_state(
        pool,
        row.id,
        "closed",
        None,
        Some(reason),
        Some(breach_price as f32),
    )
    .await?;
    debug!(
        setup_id = %row.id,
        symbol   = %row.symbol,
        tf       = %row.timeframe,
        reason,
        close_price = breach_price,
        "wyckoff setup invalidated"
    );
    Ok(true)
}

// =========================================================================
// Helpers — single-purpose (CLAUDE.md #1)
// =========================================================================

#[derive(Clone, Copy)]
enum Direction {
    Long,
    Short,
}

/// Decide which stop-breach classification applies. Returns
/// `(reason, price_at_decision)` or `None` if no breach.
///
/// Ordered so the more severe case (structural) wins over the lighter
/// (tight) one — a single bar that gaps through both should be logged
/// as `structural_invalidated` for audit clarity.
fn classify_breach(
    dir: Direction,
    close: f64,
    tight_sl: f64,
    wide_sl: Option<f64>,
    slack_ratio: f64,
) -> Option<(&'static str, f64)> {
    let slack = close * slack_ratio.abs();
    let breached_wide = match (dir, wide_sl) {
        (Direction::Long, Some(w))  => close <= w - slack,
        (Direction::Short, Some(w)) => close >= w + slack,
        _ => false,
    };
    if breached_wide {
        return Some(("structural_invalidated", close));
    }
    let breached_tight = match dir {
        Direction::Long  => close <= tight_sl - slack,
        Direction::Short => close >= tight_sl + slack,
    };
    if breached_tight {
        return Some(("sl_breach", close));
    }
    None
}

/// `raw_meta` shape per `signal_to_payload`:
/// `{ "wyckoff_classic": { "sl_wide": <f64>, ... }, ... }`.
/// Handle both nested and flat shapes defensively.
fn extract_sl_wide(meta: &JsonValue) -> Option<f64> {
    meta.get("wyckoff_classic")
        .and_then(|w| w.get("sl_wide"))
        .or_else(|| meta.get("sl_wide"))
        .and_then(|v| v.as_f64())
}

/// Reverse of `wyckoff_setup_loop::classify_venue`. Kept as a table
/// lookup so adding a new venue = one line (CLAUDE.md #1).
fn venue_to_pair(venue_class: &str) -> (&'static str, &'static str) {
    match venue_class {
        "binance_futures" => ("binance", "futures"),
        "binance_spot"    => ("binance", "spot"),
        _ => ("binance", "spot"), // safe default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_sl_breach_classified() {
        assert_eq!(
            classify_breach(Direction::Long, 99.0, 100.0, None, 0.0),
            Some(("sl_breach", 99.0))
        );
    }

    #[test]
    fn long_structural_beats_tight() {
        assert_eq!(
            classify_breach(Direction::Long, 80.0, 100.0, Some(90.0), 0.0),
            Some(("structural_invalidated", 80.0))
        );
    }

    #[test]
    fn short_slack_keeps_ok_close() {
        // 100.05 close, tight 100, slack 0.001 → 0.1 tolerance. Not breached.
        assert!(classify_breach(Direction::Short, 100.05, 100.0, None, 0.001).is_none());
    }

    #[test]
    fn short_clean_breach() {
        assert_eq!(
            classify_breach(Direction::Short, 101.0, 100.0, None, 0.0),
            Some(("sl_breach", 101.0))
        );
    }

    #[test]
    fn extract_sl_wide_nested() {
        let v: JsonValue = serde_json::from_str(
            r#"{"wyckoff_classic":{"sl_wide":98.5}}"#,
        ).unwrap();
        assert_eq!(extract_sl_wide(&v), Some(98.5));
    }
}
