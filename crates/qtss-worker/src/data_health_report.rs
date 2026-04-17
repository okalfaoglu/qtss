//! Periodic data-health report loop.
//!
//! Every `health_report.interval_hours` (default 24h) the loop gathers
//! system-wide stats via simple SQL aggregations and enqueues a Telegram
//! HTML notification through `NotifyOutboxRepository`.
//!
//! All tunables live in `system_config` (CLAUDE.md #2):
//!   - `health_report.enabled`        — master switch
//!   - `health_report.interval_hours` — sleep between runs
//!   - `health_report.channel`        — notification channel(s)

use sqlx::PgPool;
use std::time::Duration;
use tracing::{info, warn};

use qtss_storage::{
    resolve_system_string, resolve_system_u64, resolve_worker_enabled_flag,
    NotifyOutboxRepository,
};

pub async fn data_health_report_loop(pool: PgPool) {
    info!("data_health_report loop spawned");
    loop {
        if !resolve_worker_enabled_flag(
            &pool,
            "health",
            "health_report.enabled",
            "QTSS_HEALTH_REPORT_ENABLED",
            true,
        )
        .await
        {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }

        let interval_hours = resolve_system_u64(
            &pool,
            "health",
            "health_report.interval_hours",
            "QTSS_HEALTH_REPORT_INTERVAL_HOURS",
            24,
            1,
            168, // max 1 week
        )
        .await;

        if let Err(e) = run_report(&pool).await {
            warn!(%e, "data_health_report run failed");
        }

        tokio::time::sleep(Duration::from_secs(interval_hours * 3600)).await;
    }
}

// ─── Stats structs ──────────────────────────────────────────────────

struct MarketBarStats {
    count_24h: i64,
    last_bar_time: String,
}

struct DetectionStats {
    total: i64,
    confirmed: i64,
}

struct SetupStats {
    opened: i64,
    closed: i64,
    active: i64,
    avg_pnl: f64,
}

struct MlStats {
    total: i64,
    pass: i64,
    block: i64,
    shadow: i64,
    avg_score: f64,
    avg_latency: f64,
}

struct FeatureSourceRow {
    source: String,
    count: i64,
}

struct WyckoffStats {
    active: i64,
    failed_24h: i64,
}

// ─── Report runner ──────────────────────────────────────────────────

async fn run_report(pool: &PgPool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bars = fetch_market_bar_stats(pool).await?;
    let dets = fetch_detection_stats(pool).await?;
    let setups = fetch_setup_stats(pool).await?;
    let ml = fetch_ml_stats(pool).await?;
    let features = fetch_feature_source_stats(pool).await?;
    let wyckoff = fetch_wyckoff_stats(pool).await?;

    let gate_status = fetch_config_flag(pool, "ai.inference.gate_enabled").await;
    let breaker_status = fetch_config_flag(pool, "circuit_breaker.active").await;

    let channel = resolve_system_string(
        pool,
        "health",
        "health_report.channel",
        "QTSS_HEALTH_REPORT_CHANNEL",
        "telegram",
    )
    .await;

    let per_source_lines = features
        .iter()
        .map(|r| format!("• {}: {}", r.source, r.count))
        .collect::<Vec<_>>()
        .join("\n");

    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC");

    let html = format!(
        r#"<b>📊 QTSS Daily Health Report</b>

<b>Market Data</b>
• Bars (24h): {n_bars}
• Last bar: {last_bar_time}

<b>Detections (24h)</b>
• Total: {n_det} | Confirmed: {n_confirmed}

<b>Setups (24h)</b>
• Opened: {n_opened} | Closed: {n_closed} | Active: {n_active}
• Avg PnL (closed): {avg_pnl:.2}%

<b>AI Pipeline (24h)</b>
• Predictions: {n_pred} (pass: {n_pass}, block: {n_block}, shadow: {n_shadow})
• Avg score: {avg_score:.3} | Avg latency: {avg_lat:.0}ms
• Gate: {gate_status} | Breaker: {breaker_status}

<b>Feature Sources (24h snapshots)</b>
{per_source_lines}

<b>Wyckoff</b>
• Active: {n_active_wyckoff} | Failed (24h): {n_failed_wyckoff}

⏰ {timestamp}"#,
        n_bars = bars.count_24h,
        last_bar_time = bars.last_bar_time,
        n_det = dets.total,
        n_confirmed = dets.confirmed,
        n_opened = setups.opened,
        n_closed = setups.closed,
        n_active = setups.active,
        avg_pnl = setups.avg_pnl,
        n_pred = ml.total,
        n_pass = ml.pass,
        n_block = ml.block,
        n_shadow = ml.shadow,
        avg_score = ml.avg_score,
        avg_lat = ml.avg_latency,
        gate_status = gate_status,
        breaker_status = breaker_status,
        per_source_lines = per_source_lines,
        n_active_wyckoff = wyckoff.active,
        n_failed_wyckoff = wyckoff.failed_24h,
        timestamp = timestamp,
    );

    let outbox = NotifyOutboxRepository::new(pool.clone());
    outbox
        .enqueue(
            None,
            "QTSS Daily Health Report",
            &html,
            vec![channel],
        )
        .await?;

    info!("data_health_report enqueued successfully");
    Ok(())
}

// ─── SQL fetchers ───────────────────────────────────────────────────

async fn fetch_market_bar_stats(
    pool: &PgPool,
) -> Result<MarketBarStats, Box<dyn std::error::Error + Send + Sync>> {
    let row = sqlx::query_as::<_, (i64, Option<String>)>(
        r#"
        SELECT COUNT(*),
               COALESCE(TO_CHAR(MAX(open_time), 'YYYY-MM-DD HH24:MI'), '-')
        FROM market_bars
        WHERE open_time >= NOW() - INTERVAL '24 hours'
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(MarketBarStats {
        count_24h: row.0,
        last_bar_time: row.1.unwrap_or_else(|| "-".into()),
    })
}

async fn fetch_detection_stats(
    pool: &PgPool,
) -> Result<DetectionStats, Box<dyn std::error::Error + Send + Sync>> {
    let row = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT COUNT(*),
               COUNT(*) FILTER (WHERE state = 'confirmed')
        FROM qtss_v2_detections
        WHERE detected_at >= NOW() - INTERVAL '24 hours'
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(DetectionStats {
        total: row.0,
        confirmed: row.1,
    })
}

async fn fetch_setup_stats(
    pool: &PgPool,
) -> Result<SetupStats, Box<dyn std::error::Error + Send + Sync>> {
    // Opened in 24h
    let opened: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM qtss_v2_setups WHERE created_at >= NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(pool)
    .await?;

    // Closed in 24h
    let closed: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM qtss_v2_setups WHERE closed_at >= NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(pool)
    .await?;

    // Currently active
    let active: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM qtss_v2_setups WHERE state IN ('armed', 'active')",
    )
    .fetch_one(pool)
    .await?;

    // Avg PnL of closed setups via setup_outcomes
    let avg_pnl: (Option<f64>,) = sqlx::query_as(
        r#"
        SELECT AVG(pnl_pct)::float8
        FROM qtss_setup_outcomes o
        JOIN qtss_v2_setups s ON s.id = o.setup_id
        WHERE s.closed_at >= NOW() - INTERVAL '24 hours'
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(SetupStats {
        opened: opened.0,
        closed: closed.0,
        active: active.0,
        avg_pnl: avg_pnl.0.unwrap_or(0.0),
    })
}

async fn fetch_ml_stats(
    pool: &PgPool,
) -> Result<MlStats, Box<dyn std::error::Error + Send + Sync>> {
    let row = sqlx::query_as::<_, (i64, i64, i64, i64, Option<f64>, Option<f64>)>(
        r#"
        SELECT COUNT(*),
               COUNT(*) FILTER (WHERE decision = 'pass'),
               COUNT(*) FILTER (WHERE decision = 'block'),
               COUNT(*) FILTER (WHERE decision = 'shadow'),
               AVG(score)::float8,
               AVG(latency_ms)::float8
        FROM qtss_ml_predictions
        WHERE created_at >= NOW() - INTERVAL '24 hours'
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(MlStats {
        total: row.0,
        pass: row.1,
        block: row.2,
        shadow: row.3,
        avg_score: row.4.unwrap_or(0.0),
        avg_latency: row.5.unwrap_or(0.0),
    })
}

async fn fetch_feature_source_stats(
    pool: &PgPool,
) -> Result<Vec<FeatureSourceRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT source, COUNT(*)
        FROM qtss_features_snapshot
        WHERE computed_at >= NOW() - INTERVAL '24 hours'
        GROUP BY source
        ORDER BY source
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(source, count)| FeatureSourceRow { source, count })
        .collect())
}

async fn fetch_wyckoff_stats(
    pool: &PgPool,
) -> Result<WyckoffStats, Box<dyn std::error::Error + Send + Sync>> {
    let active: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM wyckoff_structures WHERE is_active = true")
            .fetch_one(pool)
            .await?;

    let failed: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM wyckoff_structures WHERE failed_at >= NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(pool)
    .await?;

    Ok(WyckoffStats {
        active: active.0,
        failed_24h: failed.0,
    })
}

async fn fetch_config_flag(pool: &PgPool, key: &str) -> String {
    let result: Result<(Option<String>,), _> = sqlx::query_as(
        "SELECT value::text FROM system_config WHERE key = $1",
    )
    .bind(key)
    .fetch_one(pool)
    .await;

    match result {
        Ok((Some(v),)) => {
            let trimmed = v.trim().trim_matches('"');
            match trimmed {
                "true" | "1" => "ON".into(),
                "false" | "0" => "OFF".into(),
                other => other.to_string(),
            }
        }
        _ => "N/A".into(),
    }
}
