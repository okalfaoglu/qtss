//! classical_backtest_sweep — Faz 12.C (classical extension).
//!
//! Same operational contract as `harmonic_backtest_sweep`, but iterates
//! the `qtss-classical::SHAPES` (+ `SHAPES_WITH_BARS`) table over sliding
//! pivot windows. For each spec we walk every `pivots_needed`-pivot
//! window in chronological order and call the spec's `eval` function
//! directly — bypassing `ClassicalDetector::detect_with_bars` which is
//! tail-only (meant for live streaming; it would only fire on the most
//! recent bar and miss the entire historical sequence we need for a
//! backtest).
//!
//! Inserts each qualifying match into `qtss_v2_detections` with:
//!
//!   * `family = 'classical'`
//!   * `mode   = 'backtest'`
//!   * `state  = 'confirmed'` (historical — the pattern either completed
//!     or not; the outcome evaluator re-examines the forward walk)
//!   * `pivot_level = 'L0'..'L3'`
//!   * `raw_meta.run_id` = UUID of this invocation (bulk rollback key)
//!
//! Env overrides (optional):
//!   * `CLASSICAL_SWEEP_SYMBOLS`    — CSV filter
//!   * `CLASSICAL_SWEEP_INTERVALS`  — CSV filter
//!   * `CLASSICAL_SWEEP_MIN_SCORE`  — structural score floor (default 0.60)

use std::env;

use chrono::{DateTime, Utc};
use qtss_classical::{ClassicalConfig, ShapeMatch, SHAPES};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::pivot::{Pivot, PivotKind, PivotLevel};
use serde_json::{json, Value as Json};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

const LEVELS: [PivotLevel; 4] = [PivotLevel::L0, PivotLevel::L1, PivotLevel::L2, PivotLevel::L3];

#[derive(Debug)]
struct SeriesKey {
    exchange: String,
    segment: String,
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
    let min_score: f32 = env::var("CLASSICAL_SWEEP_MIN_SCORE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.60);
    let symbol_filter: Option<Vec<String>> = env::var("CLASSICAL_SWEEP_SYMBOLS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let interval_filter: Option<Vec<String>> = env::var("CLASSICAL_SWEEP_INTERVALS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());

    // Default config (tolerances come from ClassicalConfig::defaults()
    // per CLAUDE.md #2 — tuning lives in config table, not code).
    let cfg = ClassicalConfig::defaults();

    tracing::info!(
        run_id = %run_id, min_score,
        symbols = ?symbol_filter, intervals = ?interval_filter,
        "classical backtest sweep starting"
    );

    let series = list_series(&pool, symbol_filter.as_deref(), interval_filter.as_deref()).await?;
    tracing::info!(count = series.len(), "series enumerated");

    let mut total: u64 = 0;
    for s in &series {
        for level in LEVELS {
            let pivots = load_pivots(&pool, s, level).await?;
            if pivots.len() < 3 {
                continue;
            }
            let n = sweep_series(&pool, s, level, &pivots, &cfg, min_score, run_id).await?;
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

async fn list_series(
    pool: &PgPool,
    symbols: Option<&[String]>,
    intervals: Option<&[String]>,
) -> anyhow::Result<Vec<SeriesKey>> {
    let rows = sqlx::query(
        r#"SELECT DISTINCT mb.exchange, mb.segment, mb.symbol, mb.interval
             FROM market_bars mb
             JOIN pivot_cache pc
               ON pc.exchange = mb.exchange
              AND pc.symbol   = mb.symbol
              AND pc.timeframe = mb.interval
            WHERE ($1::text[] IS NULL OR mb.symbol   = ANY($1))
              AND ($2::text[] IS NULL OR mb.interval = ANY($2))
            ORDER BY mb.exchange, mb.symbol, mb.interval"#,
    )
    .bind(symbols)
    .bind(intervals)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| SeriesKey {
            exchange: r.get("exchange"),
            segment: r.get("segment"),
            symbol: r.get("symbol"),
            interval: r.get("interval"),
        })
        .collect())
}

async fn load_pivots(
    pool: &PgPool,
    s: &SeriesKey,
    level: PivotLevel,
) -> anyhow::Result<Vec<Pivot>> {
    let rows = sqlx::query(
        r#"SELECT bar_index, open_time, price, kind, prominence, volume_at_pivot
             FROM pivot_cache
            WHERE exchange = $1 AND symbol = $2
              AND timeframe = $3 AND level  = $4
            ORDER BY bar_index ASC"#,
    )
    .bind(&s.exchange)
    .bind(&s.symbol)
    .bind(&s.interval)
    .bind(level.as_str())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let kind_s: String = r.get("kind");
            let kind = match kind_s.as_str() {
                "High" => PivotKind::High,
                "Low" => PivotKind::Low,
                _ => return None,
            };
            let bar_index: i64 = r.get("bar_index");
            let open_time: DateTime<Utc> = r.get("open_time");
            Some(Pivot {
                bar_index: bar_index as u64,
                time: open_time,
                price: r.get("price"),
                kind,
                level,
                prominence: r.get("prominence"),
                volume_at_pivot: r.get("volume_at_pivot"),
                swing_type: None,
            })
        })
        .collect())
}

async fn sweep_series(
    pool: &PgPool,
    s: &SeriesKey,
    level: PivotLevel,
    pivots: &[Pivot],
    cfg: &ClassicalConfig,
    min_score: f32,
    run_id: Uuid,
) -> anyhow::Result<usize> {
    // Idempotent per (symbol, interval, level): wipe prior runs.
    sqlx::query(
        r#"DELETE FROM qtss_v2_detections
            WHERE mode = 'backtest'
              AND family = 'classical'
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
    // Per-spec sliding window. Each SHAPES entry declares its own
    // `pivots_needed`, so we walk that many at a time. No central
    // match arm on pattern names — adding a new shape to the table
    // auto-extends the sweep (CLAUDE.md #1).
    for spec in SHAPES {
        let k = spec.pivots_needed;
        if pivots.len() < k {
            continue;
        }
        for start in 0..=(pivots.len() - k) {
            let win = &pivots[start..start + k];
            let Some(m) = (spec.eval)(win, cfg) else {
                continue;
            };
            if (m.score as f32) < min_score {
                continue;
            }
            let subkind = format!("{}_{}", spec.name, m.variant);
            insert_detection(pool, s, level, win, &m, &subkind, run_id).await?;
            inserted += 1;
        }
    }

    // SHAPES_WITH_BARS (cup&handle, rounding, diamond) need bar context.
    // Loading full bar history is heavy and per-shape bar_window varies;
    // omit from the backtest sweep for now — those patterns carry their
    // own dedicated analyzers in the live path. Can be added later if
    // the operator wants bar-aware shapes in the backtest inventory.

    Ok(inserted)
}

async fn insert_detection(
    pool: &PgPool,
    s: &SeriesKey,
    level: PivotLevel,
    win: &[Pivot],
    m: &ShapeMatch,
    subkind: &str,
    run_id: Uuid,
) -> anyhow::Result<()> {
    let anchors: Json = json!(win
        .iter()
        .zip(m.anchor_labels.iter())
        .map(|(p, label)| json!({
            "bar_index": p.bar_index,
            "price": p.price.to_string(),
            "level": level.as_str(),
            "label": label,
            "time": p.time.to_rfc3339(),
        }))
        .collect::<Vec<_>>());

    let raw_meta: Json = json!({
        "run_id": run_id.to_string(),
        "sweep": "classical_backtest_sweep",
        "variant": m.variant,
    });
    let regime: Json = json!({ "backtest": true });

    // detected_at = last anchor time (walk-forward correctness:
    // forward evaluation starts strictly AFTER this bar).
    let detected_at = win.last().map(|p| p.time).unwrap_or_else(Utc::now);

    sqlx::query(
        r#"INSERT INTO qtss_v2_detections (
               id, detected_at, exchange, symbol, timeframe,
               family, subkind, state, structural_score,
               invalidation_price, anchors, regime, raw_meta, mode,
               pivot_level
           ) VALUES (
               $1, $2, $3, $4, $5,
               'classical', $6, 'confirmed', $7,
               $8, $9, $10, $11, 'backtest',
               $12
           )"#,
    )
    .bind(Uuid::new_v4())
    .bind(detected_at)
    .bind(&s.exchange)
    .bind(&s.symbol)
    .bind(&s.interval)
    .bind(subkind)
    .bind(m.score as f32)
    .bind(m.invalidation)
    .bind(&anchors)
    .bind(&regime)
    .bind(&raw_meta)
    .bind(level.as_str())
    .execute(pool)
    .await?;
    let _ = s.segment.as_str();
    // Unused Bar import avoidance (Bar is needed for SHAPES_WITH_BARS
    // future extension; keep the type path resolving so the comment
    // above stays accurate).
    let _: Option<Bar> = None;
    Ok(())
}
