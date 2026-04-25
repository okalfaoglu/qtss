//! `qtss-engine` — unified pattern-family writer engine.
//!
//! Single tick loop (`engine_loop`) that dispatches to a registered list
//! of [`WriterTask`] impls. Replaces the three separate worker loops
//! (`pivot_writer_loop`, `detections_writer_loop`, `harmonic_writer_loop`)
//! with one ordered dispatch, so adding a new pattern family is a
//! one-line push into the registry (CLAUDE.md rule #1 — dispatch table,
//! not scattered `if`/`match`).
//!
//! Order of writers inside a tick matters: the harmonic matcher reads
//! the `pivots` table, so `PivotWriter` must run before `HarmonicWriter`.
//! `ElliottWriter` is independent (computes its own zigzag from bars) but
//! still benefits from running right after pivots so future cross-family
//! analyses see a consistent snapshot.
//!
//! Config (all from `system_config`, CLAUDE.md rule #2 — no hardcoded):
//!   * `engine.tick_secs` → `{ "secs": 60 }` — outer loop cadence
//!   * `engine.enabled`   → `{ "enabled": true }` — master kill switch
//!   * Per writer, its own `<family>.enabled` flag (pivot/detections/
//!     harmonic modules) gates that writer individually.

pub mod symbols;
pub mod writer;
pub mod writers;

pub use writer::{RunStats, WriterTask};

use std::time::Duration;

use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

/// Outer engine loop — starts all registered writers on a shared
/// cadence. Long-running; call via `tokio::spawn`.
pub async fn engine_loop(pool: PgPool) {
    let writers: Vec<Box<dyn WriterTask>> = registered_writers();
    info!(count = writers.len(), "qtss-engine started");
    loop {
        let master = load_master_enabled(&pool).await;
        if master {
            run_tick(&pool, &writers).await;
        } else {
            debug!("qtss-engine disabled (system_config.engine.enabled=false)");
        }
        let secs = load_tick_secs(&pool).await;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

/// One tick: iterate writers in registration order, log per-family
/// stats. A failure in one writer does not abort the rest of the tick.
async fn run_tick(pool: &PgPool, writers: &[Box<dyn WriterTask>]) {
    for w in writers {
        let family = w.family_name();
        if !w.is_enabled(pool).await {
            debug!(family, "writer disabled — skipped");
            continue;
        }
        match w.run_once(pool).await {
            Ok(stats) => info!(
                family,
                series = stats.series_processed,
                rows = stats.rows_upserted,
                "engine writer ok"
            ),
            Err(e) => warn!(family, %e, "engine writer failed"),
        }
    }
}

fn registered_writers() -> Vec<Box<dyn WriterTask>> {
    // Order is load-bearing: pivots first (harmonic + classical read from
    // pivots), elliott next (fresh snapshot aligned to the same tick),
    // harmonic + classical last so they see the pivots this tick just
    // wrote. Classical goes after harmonic purely for log readability —
    // they're independent of each other.
    vec![
        Box::new(writers::pivot::PivotWriter),
        Box::new(writers::elliott::ElliottWriter),
        // FAZ 25.x — surfaces the dormant ElliottDetectorSet (diagonal,
        // flat, extended impulse, truncated fifth, W-X-Y combination).
        // Runs after the LuxAlgo motive/abc/triangle pass so the new
        // family doesn't fight the established one over the same slot.
        Box::new(writers::elliott_full::ElliottFullWriter),
        Box::new(writers::harmonic::HarmonicWriter),
        Box::new(writers::classical::ClassicalWriter),
        Box::new(writers::range::RangeWriter),
        Box::new(writers::gap::GapWriter),
        Box::new(writers::candles::CandlesWriter),
        Box::new(writers::orb::OrbWriter),
        Box::new(writers::smc::SmcWriter),
        Box::new(writers::derivatives::DerivativesWriter),
        Box::new(writers::orderflow::OrderFlowWriter),
        Box::new(writers::wyckoff::WyckoffWriter),
    ]
}

async fn load_master_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'engine' AND config_key = 'enabled'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return true; };
    let val: serde_json::Value = row.try_get("value").unwrap_or(serde_json::Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

async fn load_tick_secs(pool: &PgPool) -> u64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'engine' AND config_key = 'tick_secs'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 60; };
    let val: serde_json::Value = row.try_get("value").unwrap_or(serde_json::Value::Null);
    val.get("secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(60)
        .max(15)
}
