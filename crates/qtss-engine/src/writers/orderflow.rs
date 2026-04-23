// Workaround: rustc 1.95 `annotate_snippets` renderer ICE on dead-code
// lint. Silenced module-wide.
#![allow(dead_code)]

//! Order-flow writer — eleventh engine-dispatch member.
//!
//! Reads the Binance WS aggregates `qtss-onchain` persists (liquidation
//! events + CVD buckets) from `data_snapshots` and publishes three
//! detection families: LiquidationCluster, BlockTrade, CVDDivergence.
//! Attached to the symbol (timeframe = '*') so the chart layer can
//! merge with any TF the user is viewing.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_orderflow::{
    detect_block_trades, detect_cvd_divergence, detect_liquidation_cluster, OrderFlowConfig,
    OrderFlowEvent, OrderFlowEventKind,
};
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct OrderFlowWriter;

#[async_trait]
impl WriterTask for OrderFlowWriter {
    fn family_name(&self) -> &'static str {
        "orderflow"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let cfg = load_config(pool).await;
        let syms = symbols::list_enabled(pool).await?;
        let mut seen = std::collections::HashSet::<String>::new();
        for sym in &syms {
            let key = format!("{}/{}/{}", sym.exchange, sym.segment, sym.symbol);
            if !seen.insert(key) {
                continue;
            }
            match process_symbol(pool, sym, &cfg).await {
                Ok(n) => {
                    stats.series_processed += 1;
                    stats.rows_upserted += n;
                }
                Err(e) => warn!(
                    exchange = %sym.exchange,
                    symbol = %sym.symbol,
                    %e,
                    "orderflow: symbol failed"
                ),
            }
        }
        Ok(stats)
    }
}

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    cfg: &OrderFlowConfig,
) -> anyhow::Result<usize> {
    let sym_lc = sym.symbol.to_lowercase();
    let exchange_lc = sym.exchange.to_lowercase();
    let mut written = 0usize;
    let now = Utc::now();

    // ── Liquidations → cluster + block trades ─────────────────────────
    if let Some(payload) =
        load_snapshot(pool, &format!("{exchange_lc}_liquidations_{sym_lc}")).await
    {
        for ev in detect_liquidation_cluster(&payload, cfg) {
            if (ev.score as f32) < cfg.min_score {
                continue;
            }
            written += write_event(pool, sym, &ev, now).await?;
        }
        for ev in detect_block_trades(&payload, cfg) {
            if (ev.score as f32) < cfg.min_score {
                continue;
            }
            written += write_event(pool, sym, &ev, now).await?;
        }
    }

    // ── CVD divergence (needs recent bar closes) ─────────────────────
    if let Some(payload) = load_snapshot(pool, &format!("{exchange_lc}_cvd_{sym_lc}")).await {
        let closes = load_recent_closes(pool, sym, 50).await;
        for ev in detect_cvd_divergence(&payload, &closes, cfg) {
            if (ev.score as f32) < cfg.min_score {
                continue;
            }
            written += write_event(pool, sym, &ev, now).await?;
        }
    }

    Ok(written)
}

async fn load_snapshot(pool: &PgPool, source_key: &str) -> Option<Value> {
    let row = sqlx::query(
        "SELECT response_json FROM data_snapshots WHERE source_key = $1 AND error IS NULL",
    )
    .bind(source_key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    row.try_get("response_json").ok()
}

async fn load_recent_closes(pool: &PgPool, sym: &EngineSymbol, limit: i64) -> Vec<f64> {
    let rows = sqlx::query(
        r#"SELECT close FROM market_bars
            WHERE exchange = $1 AND segment = $2 AND symbol = $3 AND interval = '1h'
            ORDER BY open_time DESC
            LIMIT $4"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let mut out: Vec<f64> = rows
        .into_iter()
        .filter_map(|r| r.try_get::<rust_decimal::Decimal, _>("close").ok())
        .filter_map(|d| d.to_f64())
        .collect();
    out.reverse(); // chronological
    out
}

async fn write_event(
    pool: &PgPool,
    sym: &EngineSymbol,
    ev: &OrderFlowEvent,
    now: DateTime<Utc>,
) -> anyhow::Result<usize> {
    let subkind = format!("{}_{}", ev.kind.as_str(), ev.variant);
    let direction: i16 = match ev.variant {
        "bull" => 1,
        "bear" => -1,
        _ => 0,
    };
    let anchors = json!([
        {
            "label_override": label_for(ev.kind),
            "time": now,
            "price": ev.reference_price,
        }
    ]);
    let raw_meta = json!({
        "score":     ev.score,
        "magnitude": ev.magnitude,
        "price":     ev.reference_price,
        "ts_ms":     ev.event_time_ms,
        "note":      ev.note,
        "event_kind": ev.kind.as_str(),
    });
    // Time dedup key — use `event_time_ms` when present (block trade,
    // liq cluster), else `now` (CVD divergence). Matches the ON
    // CONFLICT's (start_time, end_time) column so repeated ticks
    // within the same burst upsert into the same row.
    let event_time = if ev.event_time_ms > 0 {
        DateTime::<Utc>::from_timestamp_millis(ev.event_time_ms).unwrap_or(now)
    } else {
        now
    };
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
               direction  = EXCLUDED.direction,
               anchors    = EXCLUDED.anchors,
               raw_meta   = EXCLUDED.raw_meta,
               updated_at = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind("*")
    .bind(0i16)
    .bind("orderflow")
    .bind(&subkind)
    .bind(direction)
    .bind(0i64)
    .bind(0i64)
    .bind(event_time)
    .bind(event_time)
    .bind(&anchors)
    .bind(false)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

fn label_for(kind: OrderFlowEventKind) -> &'static str {
    match kind {
        OrderFlowEventKind::LiquidationCluster => "Liq cluster",
        OrderFlowEventKind::BlockTrade => "Block",
        OrderFlowEventKind::CvdDivergence => "CVD div",
    }
}

// ── Config ─────────────────────────────────────────────────────────────

async fn load_num(pool: &PgPool, key: &str, field: &str, default: i64) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'orderflow' AND config_key = $1",
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

async fn load_f64(pool: &PgPool, key: &str, field: &str, default: f64) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'orderflow' AND config_key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return default; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get(field).and_then(|v| v.as_f64()).unwrap_or(default)
}

async fn load_config(pool: &PgPool) -> OrderFlowConfig {
    let mut cfg = OrderFlowConfig::default();
    cfg.min_score = load_f64(pool, "min_score", "score", cfg.min_score as f64).await as f32;
    cfg.liq_cluster_min_count = load_num(
        pool,
        "thresholds.liq_cluster_min_count",
        "value",
        cfg.liq_cluster_min_count as i64,
    )
    .await as usize;
    cfg.liq_cluster_min_notional_usd = load_f64(
        pool,
        "thresholds.liq_cluster_min_notional_usd",
        "value",
        cfg.liq_cluster_min_notional_usd,
    )
    .await;
    cfg.liq_cluster_window_secs = load_num(
        pool,
        "thresholds.liq_cluster_window_secs",
        "value",
        cfg.liq_cluster_window_secs,
    )
    .await;
    cfg.block_trade_notional_usd = load_f64(
        pool,
        "thresholds.block_trade_notional_usd",
        "value",
        cfg.block_trade_notional_usd,
    )
    .await;
    cfg.cvd_divergence_bars = load_num(
        pool,
        "thresholds.cvd_divergence_bars",
        "value",
        cfg.cvd_divergence_bars as i64,
    )
    .await as usize;
    cfg.cvd_divergence_price_min_pct = load_f64(
        pool,
        "thresholds.cvd_divergence_price_min_pct",
        "value",
        cfg.cvd_divergence_price_min_pct,
    )
    .await;
    cfg.cvd_divergence_cvd_opposite_min = load_f64(
        pool,
        "thresholds.cvd_divergence_cvd_opposite_min",
        "value",
        cfg.cvd_divergence_cvd_opposite_min,
    )
    .await;
    cfg
}
