//! elliott_backtest_sweep — Faz 12.C (elliott extension).
//!
//! Re-emits every valid 5-wave impulse the `qtss-elliott` rules table
//! would have fired on historical pivots. Mirrors the harmonic/classical
//! sweep contract: idempotent per (symbol, interval, level), each match
//! persisted as a `qtss_v2_detections` row with `mode='backtest'` and
//! the source `pivot_level`.
//!
//! Implementation note: this binary mimics `ImpulseDetector::detect_all`
//! inline rather than calling it. Two reasons:
//!   1. `Detection.anchors` is `Vec<PivotRef>` which has no `time`
//!      field; we'd have to round-trip `bar_index` → `Pivot.time`.
//!   2. Building a fresh `PivotTree` per level per symbol only to
//!      throw away three of the four level slots is wasteful; the
//!      rules engine reads a flat `&[f64]` once normalized.
//!
//! The rule table (`qtss_elliott::RULES`) is the source of truth; any
//! rule added there auto-extends this sweep (CLAUDE.md #1).

use std::env;

use chrono::{DateTime, Utc};
use qtss_domain::v2::pivot::{Pivot, PivotKind, PivotLevel};
use qtss_elliott::{score_impulse, ImpulsePoints, RULES};
use rust_decimal::Decimal;
use serde_json::{json, Value as Json};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

const LEVELS: [PivotLevel; 4] = [PivotLevel::L0, PivotLevel::L1, PivotLevel::L2, PivotLevel::L3];
const LABELS: [&str; 6] = ["0", "1", "2", "3", "4", "5"];

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
    let min_score: f32 = env::var("ELLIOTT_SWEEP_MIN_SCORE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.60);
    let symbol_filter: Option<Vec<String>> = env::var("ELLIOTT_SWEEP_SYMBOLS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let interval_filter: Option<Vec<String>> = env::var("ELLIOTT_SWEEP_INTERVALS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());

    tracing::info!(
        run_id = %run_id, min_score,
        symbols = ?symbol_filter, intervals = ?interval_filter,
        "elliott backtest sweep starting"
    );

    let series = list_series(&pool, symbol_filter.as_deref(), interval_filter.as_deref()).await?;
    tracing::info!(count = series.len(), "series enumerated");

    let mut total: u64 = 0;
    for s in &series {
        for level in LEVELS {
            let pivots = load_pivots(&pool, s, level).await?;
            if pivots.len() < 6 {
                continue;
            }
            let n = sweep_series(&pool, s, level, &pivots, min_score, run_id).await?;
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
    min_score: f32,
    run_id: Uuid,
) -> anyhow::Result<usize> {
    sqlx::query(
        r#"DELETE FROM qtss_v2_detections
            WHERE mode = 'backtest'
              AND family = 'elliott'
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
    for start in 0..=(pivots.len() - 6) {
        let w = &pivots[start..start + 6];
        let bearish = w[0].kind == PivotKind::High;
        let sign = if bearish { Decimal::NEGATIVE_ONE } else { Decimal::ONE };
        let pts = points(w, sign);
        let arr = pts.as_f64();

        // Rules are predicates: all must pass. Delegate to the crate's
        // own rule table so this sweep never drifts from live semantics.
        if RULES.iter().any(|rule| rule(&arr).is_err()) {
            continue;
        }
        let score = score_impulse(&arr);
        if (score as f32) < min_score {
            continue;
        }
        let subkind = if bearish { "impulse_5_bear" } else { "impulse_5_bull" };
        insert_detection(pool, s, level, w, bearish, score as f32, subkind, run_id).await?;
        inserted += 1;
    }
    Ok(inserted)
}

fn points(w: &[Pivot], sign: Decimal) -> ImpulsePoints {
    ImpulsePoints {
        p0: w[0].price * sign,
        p1: w[1].price * sign,
        p2: w[2].price * sign,
        p3: w[3].price * sign,
        p4: w[4].price * sign,
        p5: w[5].price * sign,
    }
}

async fn insert_detection(
    pool: &PgPool,
    s: &SeriesKey,
    level: PivotLevel,
    w: &[Pivot],
    bearish: bool,
    score: f32,
    subkind: &str,
    run_id: Uuid,
) -> anyhow::Result<()> {
    // Invalidation for impulse = p0 (start of wave 1). If price
    // trades beyond p0 the count is structurally broken.
    let invalidation: Decimal = w[0].price;

    let anchors: Json = json!(w
        .iter()
        .enumerate()
        .map(|(i, p)| json!({
            "bar_index": p.bar_index,
            "price": p.price.to_string(),
            "level": level.as_str(),
            "label": LABELS[i],
            "time": p.time.to_rfc3339(),
        }))
        .collect::<Vec<_>>());

    let raw_meta: Json = json!({
        "run_id": run_id.to_string(),
        "sweep": "elliott_backtest_sweep",
        "bearish": bearish,
    });
    let regime: Json = json!({ "backtest": true });

    // detected_at = last anchor (wave 5 completion).
    let detected_at = w[5].time;

    sqlx::query(
        r#"INSERT INTO qtss_v2_detections (
               id, detected_at, exchange, symbol, timeframe,
               family, subkind, state, structural_score,
               invalidation_price, anchors, regime, raw_meta, mode,
               pivot_level
           ) VALUES (
               $1, $2, $3, $4, $5,
               'elliott', $6, 'confirmed', $7,
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
    .bind(score)
    .bind(invalidation)
    .bind(&anchors)
    .bind(&regime)
    .bind(&raw_meta)
    .bind(level.as_str())
    .execute(pool)
    .await?;
    let _ = s.segment.as_str();
    Ok(())
}
