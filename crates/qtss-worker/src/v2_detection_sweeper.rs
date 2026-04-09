//! Stale forming sweeper — Faz 7 Adım 10.
//!
//! `qtss_v2_detections` rows that never make it past `forming` (the
//! validator never picked them up, the orchestrator re-detected the
//! same pattern with a different anchor, the bar feed dried up, …)
//! would otherwise stay in the chart overlay forever. This loop ages
//! them out by flipping `state -> 'invalidated'` once they cross
//! `detection.sweeper.max_age_s`.
//!
//! Why a separate loop and not inlined in the orchestrator/validator:
//! CLAUDE.md #3 — each consumer of the table runs at its own cadence
//! and can be disabled independently. The sweep is a janitor, not a
//! decision maker; it never resurrects a row, only ages stale ones out.
//! All toggles live in `system_config` (CLAUDE.md #2).

use std::time::Duration;

use qtss_storage::{
    resolve_system_u64, resolve_worker_enabled_flag, V2DetectionRepository,
};
use sqlx::PgPool;
use tracing::{debug, info, warn};

pub async fn v2_detection_sweeper_loop(pool: PgPool) {
    info!("v2 detection sweeper loop spawned (gated on detection.sweeper.enabled)");
    let repo = V2DetectionRepository::new(pool.clone());

    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "detection",
            "sweeper.enabled",
            "QTSS_DETECTION_SWEEPER_ENABLED",
            true,
        )
        .await;

        let tick_secs = resolve_system_u64(
            &pool,
            "detection",
            "sweeper.tick_interval_s",
            "QTSS_DETECTION_SWEEPER_TICK_S",
            60,
            5,
            3600,
        )
        .await;

        if !enabled {
            tokio::time::sleep(Duration::from_secs(tick_secs)).await;
            continue;
        }

        let max_age_s = resolve_system_u64(
            &pool,
            "detection",
            "sweeper.max_age_s",
            "QTSS_DETECTION_SWEEPER_MAX_AGE_S",
            3600,
            60,
            7 * 24 * 3600,
        )
        .await as i64;

        match repo.invalidate_stale_forming(max_age_s).await {
            Ok(0) => debug!(max_age_s, "v2 detection sweeper: nothing stale"),
            Ok(n) => info!(rows = n, max_age_s, "v2 detection sweeper: invalidated stale forming rows"),
            Err(e) => warn!(%e, "v2 detection sweeper pass failed"),
        }

        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}
