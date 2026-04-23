//! Harmonic pattern writer — ports `harmonic_writer_loop` into the
//! engine dispatch. Reads pivots from the `pivots` table (populated by
//! the `PivotWriter` earlier in the same tick) and runs every 5-pivot
//! sliding window through the `qtss_harmonic::PATTERNS` catalog
//! (Gartley, Bat, Butterfly, Crab, Deep Crab, Shark, Cypher, Alt Bat,
//! 5-0, AB=CD, Alt AB=CD, Three Drives). Best-scoring match per
//! window is written into `detections` with `pattern_family='harmonic'`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_harmonic::{match_pattern, XabcdPoints, PATTERNS};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct HarmonicWriter;

#[async_trait]
impl WriterTask for HarmonicWriter {
    fn family_name(&self) -> &'static str {
        "harmonic"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let syms = symbols::list_enabled(pool).await?;
        let min_score = load_min_score(pool).await;
        let slack = load_slack(pool).await;
        let pivots_per_slot = load_num(pool, "pivots_per_slot", "count", 500)
            .await
            .clamp(50, 5_000);

        for sym in &syms {
            match process_symbol(pool, sym, pivots_per_slot, min_score, slack).await {
                Ok(n) => {
                    stats.series_processed += 1;
                    stats.rows_upserted += n;
                }
                Err(e) => warn!(
                    exchange = %sym.exchange,
                    symbol = %sym.symbol,
                    tf = %sym.interval,
                    %e,
                    "harmonic: series failed"
                ),
            }
        }
        Ok(stats)
    }
}

#[derive(Debug, Clone)]
struct StoredPivot {
    bar_index: i64,
    open_time: DateTime<Utc>,
    /// ±1 / ±2 (Pine `dir*2` strength marker). For harmonic matching
    /// we only care about the sign.
    direction: i16,
    price: Decimal,
}

async fn list_pivots_by_slot(
    pool: &PgPool,
    engine_symbol_id: sqlx::types::Uuid,
    slot: i16,
    limit: i64,
) -> anyhow::Result<Vec<StoredPivot>> {
    let rows = sqlx::query(
        r#"SELECT bar_index, open_time, direction, price
             FROM pivots
            WHERE engine_symbol_id = $1 AND level = $2
            ORDER BY bar_index DESC
            LIMIT $3"#,
    )
    .bind(engine_symbol_id)
    .bind(slot)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let mut out: Vec<StoredPivot> = rows
        .into_iter()
        .map(|r| StoredPivot {
            bar_index: r.get("bar_index"),
            open_time: r.get("open_time"),
            direction: r.get("direction"),
            price: r.get("price"),
        })
        .collect();
    out.reverse();
    Ok(out)
}

async fn load_min_score(pool: &PgPool) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'harmonic' AND config_key = 'min_score'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.60; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.60)
        .clamp(0.0, 1.0)
}

async fn load_slack(pool: &PgPool) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'harmonic' AND config_key = 'slack'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.05; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("slack")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.05)
        .clamp(0.0, 0.2)
}

async fn load_num(pool: &PgPool, key: &str, field: &str, default: i64) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'harmonic' AND config_key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return default; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get(field).and_then(|v| v.as_i64()).unwrap_or(default)
}

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    pivots_per_slot: i64,
    min_score: f64,
    slack: f64,
) -> anyhow::Result<usize> {
    let mut written = 0usize;
    for slot in 0..5i16 {
        let pivots = list_pivots_by_slot(pool, sym.id, slot, pivots_per_slot).await?;
        if pivots.len() < 5 {
            continue;
        }
        for start in 0..=(pivots.len() - 5) {
            let window = &pivots[start..start + 5];
            if !alternation_ok(window) {
                continue;
            }
            let bullish = window[0].direction < 0;
            let pts = make_xabcd(window, bullish);
            let Some((spec, score)) = best_pattern(&pts, slack) else { continue };
            if score < min_score {
                continue;
            }
            let subkind = format!("{}_{}", spec.name, if bullish { "bull" } else { "bear" });
            let direction: i16 = if bullish { 1 } else { -1 };
            let start_bar = window[0].bar_index;
            let end_bar = window[4].bar_index;
            let start_time = window[0].open_time;
            let end_time = window[4].open_time;
            let anchors = anchors_json(window);
            let raw_meta = json!({
                "score": score,
                "ratios": ratios_meta(&pts),
                "extension": spec.extension,
            });
            upsert_detection(
                pool,
                sym,
                slot,
                "harmonic",
                &subkind,
                direction,
                start_bar,
                end_bar,
                start_time,
                end_time,
                &anchors,
                false,
                raw_meta,
            )
            .await?;
            written += 1;
        }
    }
    Ok(written)
}

fn alternation_ok(window: &[StoredPivot]) -> bool {
    window
        .windows(2)
        .all(|w| w[0].direction.signum() != w[1].direction.signum())
}

fn make_xabcd(window: &[StoredPivot], bullish: bool) -> XabcdPoints {
    let p = |i: usize| -> f64 {
        let v = window[i].price.to_f64().unwrap_or(0.0);
        if bullish {
            v
        } else {
            -v
        }
    };
    XabcdPoints {
        x: p(0),
        a: p(1),
        b: p(2),
        c: p(3),
        d: p(4),
    }
}

fn best_pattern(
    pts: &XabcdPoints,
    slack: f64,
) -> Option<(&'static qtss_harmonic::HarmonicSpec, f64)> {
    let mut best_spec: Option<&'static qtss_harmonic::HarmonicSpec> = None;
    let mut best_score: f64 = f64::NEG_INFINITY;
    for spec in PATTERNS {
        if let Some(score) = match_pattern(spec, pts, slack) {
            if score > best_score {
                best_score = score;
                best_spec = Some(spec);
            }
        }
    }
    best_spec.map(|s| (s, best_score))
}

fn ratios_meta(pts: &XabcdPoints) -> Value {
    pts.ratios()
        .map(|(r_ab, r_bc, r_cd, r_ad)| {
            json!({ "ab": r_ab, "bc": r_bc, "cd": r_cd, "ad": r_ad })
        })
        .unwrap_or(Value::Null)
}

fn anchors_json(window: &[StoredPivot]) -> Value {
    const LABELS: [&str; 5] = ["X", "A", "B", "C", "D"];
    let arr: Vec<Value> = window
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "direction": p.direction.signum(),
                "bar_index": p.bar_index,
                "price": p.price.to_f64().unwrap_or(0.0),
                "time": p.open_time,
                "label_override": LABELS[i],
            })
        })
        .collect();
    Value::Array(arr)
}

#[allow(clippy::too_many_arguments)]
async fn upsert_detection(
    pool: &PgPool,
    sym: &EngineSymbol,
    slot: i16,
    family: &str,
    subkind: &str,
    direction: i16,
    start_bar: i64,
    end_bar: i64,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    anchors: &Value,
    invalidated: bool,
    raw_meta: Value,
) -> anyhow::Result<usize> {
    sqlx::query(
        r#"INSERT INTO detections
              (exchange, segment, symbol, timeframe, slot,
               pattern_family, subkind, direction,
               start_bar, end_bar, start_time, end_time,
               anchors, invalidated, raw_meta, mode)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,'live')
           ON CONFLICT (exchange, segment, symbol, timeframe, slot,
                        pattern_family, subkind, start_time, end_time, mode)
           DO UPDATE SET
               direction     = EXCLUDED.direction,
               start_bar     = EXCLUDED.start_bar,
               end_bar       = EXCLUDED.end_bar,
               anchors       = EXCLUDED.anchors,
               invalidated   = EXCLUDED.invalidated,
               raw_meta      = EXCLUDED.raw_meta,
               updated_at    = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(slot)
    .bind(family)
    .bind(subkind)
    .bind(direction)
    .bind(start_bar)
    .bind(end_bar)
    .bind(start_time)
    .bind(end_time)
    .bind(anchors)
    .bind(invalidated)
    .bind(raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}
