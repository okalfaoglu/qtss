//! pivot_reversal_backtest_sweep — Faz 13
//!
//! Thin driver: enumerates (exchange, symbol, tf, level) slices from
//! `pivot_cache`, feeds pivot vectors into `qtss_pivot_reversal::build_detection`,
//! and persists the resulting drafts into `qtss_v2_detections`
//! (mode='backtest', family='pivot_reversal').
//!
//! All structural logic, tier/event taxonomy, scoring and target
//! computation live in the `qtss-pivot-reversal` crate so that the
//! live-hook (Faz 13.5) and the outcome evaluator can reuse the
//! identical pipeline — only the persistence mode differs
//! (CLAUDE.md #5).
//!
//! Idempotent per slice: DELETE + bulk INSERT per (symbol, tf, level).

use std::env;

use qtss_pivot_reversal::{
    build_detection, features_for, DetectionDraft, PivotLevel, PivotRow, ReversalConfig,
};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

const LEVELS: [PivotLevel; 4] = [PivotLevel::L0, PivotLevel::L1, PivotLevel::L2, PivotLevel::L3];

#[derive(Debug)]
struct SeriesKey {
    exchange: String,
    symbol: String,
    interval: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let dsn = env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;
    let pool = PgPoolOptions::new().max_connections(4).connect(&dsn).await?;

    let run_id = Uuid::new_v4();
    let symbol_filter: Option<Vec<String>> = env::var("PIVOT_REVERSAL_SYMBOLS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let interval_filter: Option<Vec<String>> = env::var("PIVOT_REVERSAL_INTERVALS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());

    let cfg = ReversalConfig::load(&pool).await?;
    tracing::info!(
        run_id = %run_id,
        symbols = ?symbol_filter, intervals = ?interval_filter,
        tier_by_level = ?cfg.tier_by_level,
        prominence_floors = ?cfg.prominence_floor,
        "pivot reversal backtest sweep starting (Faz 13 tier-aware, A+B targets)"
    );

    let series = list_series(&pool, symbol_filter.as_deref(), interval_filter.as_deref()).await?;
    tracing::info!(count = series.len(), "series enumerated");

    let mut total: u64 = 0;
    for s in &series {
        for level in LEVELS {
            let pivots = load_pivots(&pool, s, level).await?;
            if pivots.len() < 2 {
                continue;
            }
            let n = sweep_series(&pool, s, level, &pivots, &cfg, run_id).await?;
            if n > 0 {
                tracing::info!(
                    symbol = %s.symbol, interval = %s.interval, level = level.as_str(),
                    pivots = pivots.len(), inserted = n,
                    "sweep batch done"
                );
                total += n as u64;
            }
        }
    }
    tracing::info!(total_inserted = total, run_id = %run_id, "sweep complete");
    Ok(())
}

fn pivot_level_to_i16(level: PivotLevel) -> i16 {
    match level {
        PivotLevel::L0 => 0,
        PivotLevel::L1 => 1,
        PivotLevel::L2 => 2,
        PivotLevel::L3 => 3,
        PivotLevel::L4 => 4,
    }
}

async fn list_series(
    pool: &PgPool,
    symbols: Option<&[String]>,
    intervals: Option<&[String]>,
) -> anyhow::Result<Vec<SeriesKey>> {
    let rows = sqlx::query(
        r#"SELECT DISTINCT es.exchange, es.symbol, es."interval" AS interval
             FROM pivots p
             JOIN engine_symbols es ON es.id = p.engine_symbol_id
            WHERE ($1::text[] IS NULL OR es.symbol     = ANY($1))
              AND ($2::text[] IS NULL OR es."interval" = ANY($2))
            ORDER BY 1, 2, 3"#,
    )
    .bind(symbols)
    .bind(intervals)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| SeriesKey {
            exchange: r.get("exchange"),
            symbol: r.get("symbol"),
            interval: r.get("interval"),
        })
        .collect())
}

async fn load_pivots(
    pool: &PgPool,
    s: &SeriesKey,
    level: PivotLevel,
) -> anyhow::Result<Vec<PivotRow>> {
    let level_i: i16 = pivot_level_to_i16(level);
    let rows = sqlx::query(
        r#"SELECT p.bar_index, p.open_time, p.price, p.direction,
                  p.prominence, p.swing_tag
             FROM pivots p
             JOIN engine_symbols es ON es.id = p.engine_symbol_id
            WHERE es.exchange   = $1
              AND es.symbol     = $2
              AND es."interval" = $3
              AND p.level       = $4
            ORDER BY p.bar_index ASC"#,
    )
    .bind(&s.exchange)
    .bind(&s.symbol)
    .bind(&s.interval)
    .bind(level_i)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let prominence: Decimal = r.get("prominence");
            let direction: i16 = r.get("direction");
            PivotRow {
                bar_index: r.get("bar_index"),
                open_time: r.get("open_time"),
                price: r.get("price"),
                kind: if direction >= 1 { "High".to_string() } else { "Low".to_string() },
                prominence: prominence.to_f64(),
                swing_type: r.get("swing_tag"),
            }
        })
        .collect())
}

async fn sweep_series(
    pool: &PgPool,
    s: &SeriesKey,
    level: PivotLevel,
    pivots: &[PivotRow],
    cfg: &ReversalConfig,
    run_id: Uuid,
) -> anyhow::Result<usize> {
    // Idempotent per slice.
    sqlx::query(
        r#"DELETE FROM qtss_v2_detections
            WHERE mode = 'backtest'
              AND family = 'pivot_reversal'
              AND exchange = $1 AND symbol = $2
              AND timeframe = $3 AND pivot_level = $4"#,
    )
    .bind(&s.exchange)
    .bind(&s.symbol)
    .bind(&s.interval)
    .bind(level.as_str())
    .execute(pool)
    .await?;

    let mut inserted = 0usize;
    for i in 2..pivots.len() {
        let Some(draft) = build_detection(pivots, i, level, cfg) else {
            continue;
        };
        insert_detection(pool, s, &draft, run_id).await?;
        inserted += 1;
    }
    Ok(inserted)
}

async fn insert_detection(
    pool: &PgPool,
    s: &SeriesKey,
    draft: &DetectionDraft,
    run_id: Uuid,
) -> anyhow::Result<()> {
    // Stamp run_id + sweep source into raw_meta without losing the
    // detector's payload (targets + tier + structural labels).
    let mut raw_meta = draft.raw_meta.clone();
    if let Some(obj) = raw_meta.as_object_mut() {
        obj.insert(
            "run_id".to_string(),
            serde_json::Value::String(run_id.to_string()),
        );
        obj.insert(
            "sweep".to_string(),
            serde_json::Value::String("pivot_reversal_backtest_sweep".to_string()),
        );
    }
    let regime = serde_json::json!({ "backtest": true });

    let detection_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO qtss_v2_detections (
               id, detected_at, exchange, symbol, timeframe,
               family, subkind, state, structural_score,
               invalidation_price, anchors, regime, raw_meta, mode,
               pivot_level
           ) VALUES (
               $1, $2, $3, $4, $5,
               'pivot_reversal', $6, 'confirmed', $7,
               $8, $9, $10, $11, 'backtest',
               $12
           )"#,
    )
    .bind(detection_id)
    .bind(draft.detected_at)
    .bind(&s.exchange)
    .bind(&s.symbol)
    .bind(&s.interval)
    .bind(&draft.subkind)
    .bind(draft.structural_score)
    .bind(draft.invalidation_price)
    .bind(&draft.anchors)
    .bind(&regime)
    .bind(&raw_meta)
    .bind(draft.pivot_level.as_str())
    .execute(pool)
    .await?;

    // Faz 13 — AI training_set besleme. features_for() payload'ını
    // `qtss_features_snapshot` ile detection'a bağla. Setup Engine
    // bu detection'dan setup açarsa v_qtss_training_set otomatik
    // ilişkiyi kurar. Setup açılmazsa da feature satırı arşivde kalır.
    let features_json = features_for(draft);
    sqlx::query(
        r#"INSERT INTO qtss_features_snapshot
               (detection_id, exchange, symbol, timeframe,
                source, feature_spec_version, features_json, meta_json)
           VALUES ($1, $2, $3, $4, 'pivot_reversal', 1, $5, $6)
           ON CONFLICT (detection_id, source, feature_spec_version) DO NOTHING"#,
    )
    .bind(detection_id)
    .bind(&s.exchange)
    .bind(&s.symbol)
    .bind(&s.interval)
    .bind(&features_json)
    .bind(serde_json::json!({
        "run_id": run_id.to_string(),
        "tier":   draft.tier.as_str(),
        "event":  draft.event.event_tag(),
        "level":  draft.pivot_level.as_str(),
    }))
    .execute(pool)
    .await?;
    Ok(())
}
