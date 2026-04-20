//! harmonic_backtest_sweep — Faz 12.C
//!
//! Offline sweep binary. For every (exchange, symbol, interval) pair
//! present in `market_bars`, pulls the full `pivot_cache` series per
//! level (L0..L3) and enumerates every 5-pivot sliding window for a
//! match against the harmonic pattern table (Gartley, Bat, Butterfly,
//! Crab, Deep Crab, Shark, Cypher, Alt-Bat, 5-0, AB=CD, Alt-AB=CD).
//! Every match whose structural score clears the detector threshold is
//! written to `qtss_v2_detections` with:
//!
//!   * `mode = 'backtest'`
//!   * `state = 'confirmed'` (historical — D pivot is final)
//!   * `pivot_level` = level the pattern was mined from
//!   * `raw_meta.run_id` = this invocation's UUID so a later sweep can
//!     be bulk-deleted without touching unrelated rows
//!
//! The outcome evaluator (Faz 12.D) later attaches `pnl_pct` / outcome
//! via `qtss_v2_detection_outcomes` using forward-walk on `market_bars`.
//!
//! This binary deliberately bypasses `HarmonicDetector::detect()` which
//! returns only the globally-best match per tree — for backtest we need
//! every qualifying window. The matcher/spec-table from `qtss-harmonic`
//! is reused unchanged; only the orchestration differs.
//!
//! Usage (from repo root):
//!
//! ```sh
//! DATABASE_URL=postgres://… cargo run --release \
//!     -p qtss-worker --bin harmonic-backtest-sweep
//! ```
//!
//! Optional env:
//!   * `HARMONIC_SWEEP_SYMBOLS="BTCUSDT,ETHUSDT"` — limit scan set
//!   * `HARMONIC_SWEEP_INTERVALS="4h,1d,1w"` — limit TFs
//!   * `HARMONIC_SWEEP_MIN_SCORE="0.70"` — override min structural score
//!   * `HARMONIC_SWEEP_SLACK="0.08"` — override ratio slack

use std::env;

use chrono::{DateTime, Utc};
use qtss_harmonic::{match_pattern, XabcdPoints, PATTERNS};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value as Json};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

const LEVELS: [&str; 4] = ["L0", "L1", "L2", "L3"];

#[derive(Debug)]
struct PivotRow {
    bar_index: i64,
    open_time: DateTime<Utc>,
    price: Decimal,
    kind: String, // "High" | "Low"
}

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

    let dsn = env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&dsn)
        .await?;

    let run_id = Uuid::new_v4();
    let min_score: f32 = env::var("HARMONIC_SWEEP_MIN_SCORE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.60);
    let slack: f64 = env::var("HARMONIC_SWEEP_SLACK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.08);
    let symbol_filter: Option<Vec<String>> = env::var("HARMONIC_SWEEP_SYMBOLS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let interval_filter: Option<Vec<String>> = env::var("HARMONIC_SWEEP_INTERVALS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());

    tracing::info!(
        run_id = %run_id, min_score, slack,
        symbols = ?symbol_filter, intervals = ?interval_filter,
        "harmonic backtest sweep starting"
    );

    let series = list_series(&pool, symbol_filter.as_deref(), interval_filter.as_deref()).await?;
    tracing::info!(count = series.len(), "series enumerated");

    let mut total: u64 = 0;
    for s in &series {
        for level in LEVELS {
            let pivots = load_pivots(&pool, s, level).await?;
            if pivots.len() < 5 {
                continue;
            }
            let n = sweep_series(&pool, s, level, &pivots, min_score, slack, run_id).await?;
            if n > 0 {
                tracing::info!(
                    symbol = %s.symbol, interval = %s.interval, level,
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
    // Distinct (exchange, segment, symbol, interval) tuples that have
    // at least one pivot row — feeding the harmonic scan on series
    // without any L0 pivots is wasted work.
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
    level: &str,
) -> anyhow::Result<Vec<PivotRow>> {
    let rows = sqlx::query(
        r#"SELECT bar_index, open_time, price, kind
             FROM pivot_cache
            WHERE exchange = $1 AND symbol = $2
              AND timeframe = $3 AND level  = $4
            ORDER BY bar_index ASC"#,
    )
    .bind(&s.exchange)
    .bind(&s.symbol)
    .bind(&s.interval)
    .bind(level)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| PivotRow {
            bar_index: r.get("bar_index"),
            open_time: r.get("open_time"),
            price: r.get("price"),
            kind: r.get("kind"),
        })
        .collect())
}

async fn sweep_series(
    pool: &PgPool,
    s: &SeriesKey,
    level: &str,
    pivots: &[PivotRow],
    min_score: f32,
    slack: f64,
    run_id: Uuid,
) -> anyhow::Result<usize> {
    // Idempotent per (symbol, interval, level): clear any prior
    // backtest rows from earlier runs so totals don't double-count.
    sqlx::query(
        r#"DELETE FROM qtss_v2_detections
            WHERE mode = 'backtest'
              AND family = 'harmonic'
              AND exchange = $1 AND symbol = $2
              AND timeframe = $3 AND pivot_level = $4"#,
    )
    .bind(&s.exchange)
    .bind(&s.symbol)
    .bind(&s.interval)
    .bind(level)
    .execute(pool)
    .await?;

    let mut inserted = 0usize;
    // Sliding 5-pivot window. Same geometry as
    // `HarmonicDetector::detect` but emits EVERY qualifying match, not
    // just the global best — backtest needs full signal inventory.
    for start in 0..=(pivots.len().saturating_sub(5)) {
        let w = &pivots[start..start + 5];
        // Bullish pattern starts at a low (X is a valley); bearish at
        // a high. Skip windows that don't alternate low/high cleanly —
        // the ATR ZigZag engine guarantees alternation, so this is
        // just a cheap belt-and-braces check.
        if !alternates(w) {
            continue;
        }
        let bearish = w[0].kind == "High";
        let sign = if bearish { -1.0 } else { 1.0 };
        let pts = match points(w, sign) {
            Some(p) => p,
            None => continue,
        };

        for spec in PATTERNS {
            let Some(score) = match_pattern(spec, &pts, slack) else {
                continue;
            };
            if (score as f32) < min_score {
                continue;
            }
            let subkind = format!(
                "{}_{}",
                spec.name,
                if bearish { "bear" } else { "bull" }
            );
            insert_detection(pool, s, level, w, spec.extension, bearish, score as f32, &subkind, run_id)
                .await?;
            inserted += 1;
        }
    }
    Ok(inserted)
}

fn alternates(w: &[PivotRow]) -> bool {
    w.windows(2).all(|pair| pair[0].kind != pair[1].kind)
}

fn points(w: &[PivotRow], sign: f64) -> Option<XabcdPoints> {
    Some(XabcdPoints {
        x: w[0].price.to_f64()? * sign,
        a: w[1].price.to_f64()? * sign,
        b: w[2].price.to_f64()? * sign,
        c: w[3].price.to_f64()? * sign,
        d: w[4].price.to_f64()? * sign,
    })
}

#[allow(clippy::too_many_arguments)]
async fn insert_detection(
    pool: &PgPool,
    s: &SeriesKey,
    level: &str,
    w: &[PivotRow],
    is_extension: bool,
    bearish: bool,
    score: f32,
    subkind: &str,
    run_id: Uuid,
) -> anyhow::Result<()> {
    // Invalidation = SL anchor, matching HarmonicDetector::detect():
    //   retracement patterns (is_extension=false) → X pivot price
    //   extension patterns → D pivot ± 2% of XA
    let d_price = w[4].price;
    let x_price = w[0].price;
    let xa = (w[1].price - x_price).abs();
    let buffer: Decimal = {
        let two_pct = Decimal::new(2, 2); // 0.02
        two_pct * xa
    };
    let invalidation = match (bearish, is_extension) {
        (false, true) => d_price - buffer,
        (true,  true) => d_price + buffer,
        (_,     false) => x_price,
    };

    const XABCD: [&str; 5] = ["X", "A", "B", "C", "D"];
    let anchors = json!(w
        .iter()
        .enumerate()
        .map(|(i, p)| json!({
            "bar_index": p.bar_index,
            "price": p.price.to_string(),
            "level": level,
            "label": XABCD[i],
            "time": p.open_time.to_rfc3339(),
        }))
        .collect::<Vec<_>>());

    let raw_meta: Json = json!({
        "run_id": run_id.to_string(),
        "sweep": "harmonic_backtest_sweep",
        "bearish": bearish,
    });

    // Minimal regime snapshot — full regime is costly to recompute on
    // backtest and the outcome evaluator doesn't need it. Keep the
    // column non-null so downstream readers don't explode.
    let regime: Json = json!({ "backtest": true });

    // `detected_at` = D pivot time. That's the only timestamp that
    // makes the walk-forward evaluator's window correct: entry = D
    // close, forward simulation starts from bars AFTER D.
    let detected_at = w[4].open_time;

    sqlx::query(
        r#"INSERT INTO qtss_v2_detections (
               id, detected_at, exchange, symbol, timeframe,
               family, subkind, state, structural_score,
               invalidation_price, anchors, regime, raw_meta, mode,
               pivot_level
           ) VALUES (
               $1, $2, $3, $4, $5,
               'harmonic', $6, 'confirmed', $7,
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
    .bind(level)
    .execute(pool)
    .await?;
    let _ = s.segment.as_str(); // segment carried for future per-segment reports
    Ok(())
}
