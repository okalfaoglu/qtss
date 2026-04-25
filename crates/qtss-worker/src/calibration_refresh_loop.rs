// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `calibration_refresh_loop` — v1.2.4.
//!
//! 0253 promoted `v_confidence_calibration` from a plain VIEW to a
//! MATERIALIZED VIEW. Materialised views don't refresh themselves —
//! something has to call `REFRESH MATERIALIZED VIEW`. This loop is
//! that something. Tick cadence + on/off live in
//! `system_config.calibration_refresh.*`.
//!
//! `CONCURRENTLY` is used because the allocator reads this matview on
//! every candidate tick — a non-concurrent refresh would briefly lock
//! reads. The unique index on `bucket` (added in 0253) is what makes
//! `CONCURRENTLY` legal.

use std::time::Duration;

use serde_json::Value;
use sqlx::{PgPool, Row};
use tracing::{info, warn};

pub async fn calibration_refresh_loop(pool: PgPool) {
    info!("calibration_refresh_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            continue;
        }
        match sqlx::query(
            "REFRESH MATERIALIZED VIEW CONCURRENTLY v_confidence_calibration",
        )
        .execute(&pool)
        .await
        {
            Ok(_) => info!("calibration_refresh: ok"),
            Err(e) => warn!(%e, "calibration_refresh: failed"),
        }
        let secs = load_tick_secs(&pool).await;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='calibration_refresh' AND config_key='enabled'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return true; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

async fn load_tick_secs(pool: &PgPool) -> u64 {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='calibration_refresh' AND config_key='tick_secs'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 600; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("secs").and_then(|v| v.as_u64()).unwrap_or(600).max(60)
}
