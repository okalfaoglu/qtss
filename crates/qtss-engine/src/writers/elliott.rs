//! Elliott pattern writer — ports `detections_writer_loop` into the
//! engine dispatch. Mirrors the `/v2/elliott` endpoint bit-for-bit
//! (same LuxAlgo Pine port + same Flat/Triangle classifier + same
//! wave-4 containment filter) and persists motives / ABCs / triangles
//! into the `detections` table with `pattern_family` ∈
//! {`motive`, `abc`, `triangle`}.

use async_trait::async_trait;
use qtss_elliott::luxalgo_pine_port::{
    self as pine, AbcPattern, Bar as PineBar, LevelConfig, LevelOutput, MotivePattern,
    PinePortConfig, PinePortOutput, TrianglePattern,
};
use qtss_storage::market_bars;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct ElliottWriter;

#[async_trait]
impl WriterTask for ElliottWriter {
    fn family_name(&self) -> &'static str {
        // Legacy config module name so existing
        // `system_config.detections.enabled` rows keep gating this
        // writer. (CLAUDE.md #2 — config is the contract, don't break it.)
        "detections"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let syms = symbols::list_enabled(pool).await?;
        let bars_limit = load_bars_per_tick(pool).await;
        let lengths = symbols::load_slot_lengths(pool).await;

        for sym in &syms {
            match process_symbol(pool, sym, bars_limit, &lengths).await {
                Ok(n) => {
                    stats.series_processed += 1;
                    stats.rows_upserted += n;
                }
                Err(e) => warn!(
                    exchange = %sym.exchange,
                    symbol = %sym.symbol,
                    tf = %sym.interval,
                    %e,
                    "elliott: series failed"
                ),
            }
        }
        Ok(stats)
    }
}

async fn load_bars_per_tick(pool: &PgPool) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module = 'detections' AND config_key = 'bars_per_tick'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 2000; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("bars")
        .and_then(|v| v.as_i64())
        .unwrap_or(2000)
        .clamp(200, 10_000)
}

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    bars_limit: i64,
    lengths: &[u32; 5],
) -> anyhow::Result<usize> {
    let raw = market_bars::list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        bars_limit,
    )
    .await?;
    if raw.len() < 50 {
        return Ok(0);
    }
    // Chronological order — same convention as /v2/elliott.
    let chrono: Vec<_> = raw.into_iter().rev().collect();

    let pine_bars: Vec<PineBar> = chrono
        .iter()
        .map(|r| PineBar {
            open: r.open.to_f64().unwrap_or(0.0),
            high: r.high.to_f64().unwrap_or(0.0),
            low: r.low.to_f64().unwrap_or(0.0),
            close: r.close.to_f64().unwrap_or(0.0),
        })
        .collect();

    let palette: [&str; 5] = ["#ef4444", "#3b82f6", "#e5e7eb", "#f59e0b", "#a78bfa"];
    let cfg = PinePortConfig {
        levels: lengths
            .iter()
            .enumerate()
            .map(|(i, &length)| LevelConfig {
                length: length as usize,
                color: palette[i % palette.len()].to_string(),
            })
            .collect(),
        ..PinePortConfig::default()
    };

    let pine_out: PinePortOutput = pine::run(&pine_bars, &cfg);

    let mut written = 0usize;
    for (slot_idx, level) in pine_out.levels.iter().enumerate() {
        for motive in &level.motives {
            written += write_motive(pool, sym, slot_idx as i16, motive, &chrono, level).await?;
            if let Some(abc) = &motive.abc {
                written += write_abc(pool, sym, slot_idx as i16, motive, abc, &chrono).await?;
            }
        }
        for tri in &level.triangles {
            written += write_triangle(pool, sym, slot_idx as i16, tri, &chrono).await?;
        }
    }
    Ok(written)
}

async fn write_motive(
    pool: &PgPool,
    sym: &EngineSymbol,
    slot: i16,
    motive: &MotivePattern,
    chrono: &[qtss_storage::market_bars::MarketBarRow],
    level: &LevelOutput,
) -> anyhow::Result<usize> {
    let start_bar = motive.anchors[0].bar_index;
    let end_bar = motive.anchors[5].bar_index;
    let (start_time, end_time) = anchor_time_range(chrono, start_bar, end_bar);
    let meta = json!({
        "break_box": motive.break_box,
        "next_marker": motive.next_marker,
        "fib_band": level.fib_band,
    });
    upsert(
        pool,
        sym,
        slot,
        "motive",
        "impulse",
        motive.direction as i16,
        start_bar,
        end_bar,
        start_time,
        end_time,
        &anchors_with_times(&motive.anchors, chrono),
        Some(motive.live),
        Some(motive.next_hint),
        false,
        meta,
    )
    .await
}

async fn write_abc(
    pool: &PgPool,
    sym: &EngineSymbol,
    slot: i16,
    motive: &MotivePattern,
    abc: &AbcPattern,
    chrono: &[qtss_storage::market_bars::MarketBarRow],
) -> anyhow::Result<usize> {
    let subkind = abc
        .subkind
        .clone()
        .unwrap_or_else(|| "zigzag".to_string());
    let start_bar = abc.anchors[0].bar_index;
    let end_bar = abc.anchors[3].bar_index;
    let (start_time, end_time) = anchor_time_range(chrono, start_bar, end_bar);
    let meta = json!({ "parent_motive_dir": motive.direction });
    upsert(
        pool,
        sym,
        slot,
        "abc",
        &subkind,
        abc.direction as i16,
        start_bar,
        end_bar,
        start_time,
        end_time,
        &anchors_with_times(&abc.anchors, chrono),
        None,
        None,
        abc.invalidated,
        meta,
    )
    .await
}

async fn write_triangle(
    pool: &PgPool,
    sym: &EngineSymbol,
    slot: i16,
    tri: &TrianglePattern,
    chrono: &[qtss_storage::market_bars::MarketBarRow],
) -> anyhow::Result<usize> {
    let start_bar = tri.anchors[0].bar_index;
    let end_bar = tri.anchors[5].bar_index;
    let (start_time, end_time) = anchor_time_range(chrono, start_bar, end_bar);
    upsert(
        pool,
        sym,
        slot,
        "triangle",
        &tri.subkind,
        tri.direction as i16,
        start_bar,
        end_bar,
        start_time,
        end_time,
        &anchors_with_times(&tri.anchors, chrono),
        None,
        None,
        tri.invalidated,
        json!({}),
    )
    .await
}

fn anchors_with_times(
    anchors: &[qtss_elliott::luxalgo_pine_port::PivotPoint],
    chrono: &[qtss_storage::market_bars::MarketBarRow],
) -> Value {
    let arr: Vec<Value> = anchors
        .iter()
        .map(|a| {
            let time = chrono
                .get(a.bar_index.max(0) as usize)
                .map(|r| r.open_time);
            let mut obj = json!({
                "direction": a.direction,
                "bar_index": a.bar_index,
                "price": a.price,
            });
            if let Some(t) = time {
                obj["time"] = json!(t);
            }
            if let Some(lo) = &a.label_override {
                obj["label_override"] = json!(lo);
            }
            if a.hide_label {
                obj["hide_label"] = json!(true);
            }
            obj
        })
        .collect();
    Value::Array(arr)
}

fn anchor_time_range(
    chrono: &[qtss_storage::market_bars::MarketBarRow],
    start_bar: i64,
    end_bar: i64,
) -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
    let clamp = |b: i64| -> Option<chrono::DateTime<chrono::Utc>> {
        chrono.get(b.max(0) as usize).map(|r| r.open_time)
    };
    let start = clamp(start_bar)
        .or_else(|| chrono.first().map(|r| r.open_time))
        .unwrap_or_else(chrono::Utc::now);
    let end = clamp(end_bar)
        .or_else(|| chrono.last().map(|r| r.open_time))
        .unwrap_or_else(chrono::Utc::now);
    (start.min(end), start.max(end))
}

#[allow(clippy::too_many_arguments)]
async fn upsert(
    pool: &PgPool,
    sym: &EngineSymbol,
    slot: i16,
    family: &str,
    subkind: &str,
    direction: i16,
    start_bar: i64,
    end_bar: i64,
    start_time: chrono::DateTime<chrono::Utc>,
    end_time: chrono::DateTime<chrono::Utc>,
    anchors: &Value,
    live: Option<bool>,
    next_hint: Option<bool>,
    invalidated: bool,
    raw_meta: Value,
) -> anyhow::Result<usize> {
    sqlx::query(
        r#"INSERT INTO detections
              (exchange, segment, symbol, timeframe, slot,
               pattern_family, subkind, direction,
               start_bar, end_bar, start_time, end_time,
               anchors, live, next_hint, invalidated, raw_meta, mode)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,'live')
           ON CONFLICT (exchange, segment, symbol, timeframe, slot,
                        pattern_family, subkind, start_time, end_time, mode)
           DO UPDATE SET
               direction     = EXCLUDED.direction,
               start_bar     = EXCLUDED.start_bar,
               end_bar       = EXCLUDED.end_bar,
               anchors       = EXCLUDED.anchors,
               live          = EXCLUDED.live,
               next_hint     = EXCLUDED.next_hint,
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
    .bind(live)
    .bind(next_hint)
    .bind(invalidated)
    .bind(raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}
