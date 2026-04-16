//! Periodic refresh of `qtss_v2_detection_outcome_stats` (migration 0107).
//!
//! The materialized view caches the `family/subkind/timeframe` outcome
//! aggregate used by the detection-validator's hit-rate proxy. Doing
//! the `COUNT(*) GROUP BY` on the 11M-row base table every tick was
//! costing ~1.2 s per call (sqlx "slow statement" warn). A
//! `REFRESH MATERIALIZED VIEW CONCURRENTLY` every few minutes is
//! more than fresh enough — the validator uses it only when no real
//! outcome exists for a pattern yet (fallback path).

use std::time::Duration;
use sqlx::PgPool;
use tracing::{debug, warn};

use qtss_storage::{resolve_system_u64, resolve_worker_enabled_flag};

/// Spawns forever. Safe to run concurrently with readers — we use
/// `REFRESH MATERIALIZED VIEW CONCURRENTLY`.
pub async fn detection_stats_refresh_loop(pool: PgPool) {
    // Defaults intentionally generous — the MV isn't latency-sensitive.
    const DEFAULT_INTERVAL_SECS: u64 = 300; // 5 min

    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool, "worker", "detection_stats_refresh.enabled", "", true,
        )
        .await;
        let tick_secs = resolve_system_u64(
            &pool,
            "worker",
            "detection_stats_refresh.interval_seconds",
            "",
            DEFAULT_INTERVAL_SECS,
            60,
            3600,
        )
        .await;

        if !enabled {
            tokio::time::sleep(Duration::from_secs(tick_secs)).await;
            continue;
        }

        // CONCURRENTLY requires the unique index created in migration 0107.
        // Fall back to plain REFRESH on cold-start (MV has no rows yet and
        // the CONCURRENTLY variant would error).
        let sql = "REFRESH MATERIALIZED VIEW CONCURRENTLY qtss_v2_detection_outcome_stats";
        match sqlx::query(sql).execute(&pool).await {
            Ok(_) => debug!("detection_stats_refresh: MV refreshed"),
            Err(e) => {
                warn!(%e, "detection_stats_refresh: concurrent refresh failed; retrying plain");
                let fallback = "REFRESH MATERIALIZED VIEW qtss_v2_detection_outcome_stats";
                if let Err(e2) = sqlx::query(fallback).execute(&pool).await {
                    warn!(%e2, "detection_stats_refresh: plain refresh also failed");
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}
