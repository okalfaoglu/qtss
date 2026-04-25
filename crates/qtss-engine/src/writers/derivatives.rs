// Workaround: rustc 1.95 `annotate_snippets` renderer ICE on dead-code
// lint. Silenced module-wide.
#![allow(dead_code)]

//! Derivatives-signals writer — tenth engine-dispatch member.
//!
//! Reads the per-symbol `data_snapshots` rows `qtss-onchain` already
//! populates (funding rate history, open interest history, premium
//! index, long/short ratio, taker buy-sell ratio) and calls the
//! detectors in `qtss-derivatives-signals`. Each qualifying event is
//! upserted into `detections` with `pattern_family = 'derivatives'`
//! and `subkind = <kind>_<variant>`.
//!
//! Detections are per-symbol, not per-timeframe (funding/OI/basis
//! don't have a native TF). We tag them as `timeframe = '*'` so the
//! chart reads them on any TF the user is viewing.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_derivatives_signals::{
    detect_basis_dislocation, detect_funding_spike, detect_long_short_extreme,
    detect_oi_imbalance, detect_taker_flow_imbalance, DerivConfig, DerivEvent, DerivEventKind,
};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct DerivativesWriter;

#[async_trait]
impl WriterTask for DerivativesWriter {
    fn family_name(&self) -> &'static str {
        "derivatives"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let cfg = load_config(pool).await;
        // Distinct symbols from engine_symbols (we don't need per-TF
        // iteration — derivatives are TF-agnostic).
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
                    "derivatives: symbol failed"
                ),
            }
        }
        Ok(stats)
    }
}

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    cfg: &DerivConfig,
) -> anyhow::Result<usize> {
    let sym_lc = sym.symbol.to_lowercase();
    let exchange_lc = sym.exchange.to_lowercase();
    let mut written = 0usize;
    let now = Utc::now();

    // ── FundingSpike ──────────────────────────────────────────────────
    if let Some(payload) =
        load_snapshot(pool, &format!("{exchange_lc}_funding_rate_{sym_lc}")).await
    {
        for ev in detect_funding_spike(&payload, cfg) {
            if (ev.score as f32) < cfg.min_score {
                continue;
            }
            written += write_event(pool, sym, &ev, now).await?;
        }
    }

    // ── BasisDislocation ──────────────────────────────────────────────
    if let Some(payload) = load_snapshot(pool, &format!("{exchange_lc}_premium_{sym_lc}")).await {
        for ev in detect_basis_dislocation(&payload, cfg) {
            if (ev.score as f32) < cfg.min_score {
                continue;
            }
            written += write_event(pool, sym, &ev, now).await?;
        }
    }

    // ── LongShortRatio ────────────────────────────────────────────────
    // qtss-onchain writes this under `binance_ls_ratio_<sym>` (see
    // engines::external_binance). The Binance endpoint is
    // `/futures/data/globalLongShortAccountRatio` — payload shape
    // matches `detect_long_short_extreme`.
    if let Some(payload) = load_snapshot(pool, &format!("{exchange_lc}_ls_ratio_{sym_lc}")).await
    {
        for ev in detect_long_short_extreme(&payload, cfg) {
            if (ev.score as f32) < cfg.min_score {
                continue;
            }
            written += write_event(pool, sym, &ev, now).await?;
        }
    }

    // ── TakerFlowImbalance ────────────────────────────────────────────
    // qtss-onchain registers this as `binance_taker_<sym>` (poll of
    // `/futures/data/takerlongshortRatio`, period=5m, every 60s). Earlier
    // versions of this writer used `..._taker_ratio_...` which silently
    // mismatched the live key — fixed v1.2.3.
    if let Some(payload) = load_snapshot(pool, &format!("{exchange_lc}_taker_{sym_lc}")).await {
        for ev in detect_taker_flow_imbalance(&payload, cfg) {
            if (ev.score as f32) < cfg.min_score {
                continue;
            }
            written += write_event(pool, sym, &ev, now).await?;
        }
    }

    // ── OIImbalance ───────────────────────────────────────────────────
    // Requires both OI history and a recent price delta. We use the
    // 1h timeframe kline's last 24 bars to approximate the window.
    if let Some(oi_payload) =
        load_snapshot(pool, &format!("{exchange_lc}_open_interest_{sym_lc}")).await
    {
        // Approximate 24h price delta from the most-granular
        // market_bars we have for this symbol (futures default TF).
        let price_delta = estimate_price_delta_24h(pool, sym).await;
        for ev in detect_oi_imbalance(&oi_payload, price_delta, cfg) {
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

async fn estimate_price_delta_24h(pool: &PgPool, sym: &EngineSymbol) -> f64 {
    // Fetch the most recent 25 bars of 1h data for a cheap 24h-delta
    // approximation. Falls back to 0 if the symbol's 1h stream isn't
    // available (which suppresses OI-imbalance events on that symbol).
    let row = sqlx::query(
        r#"SELECT close FROM market_bars
            WHERE exchange = $1 AND segment = $2 AND symbol = $3 AND interval = '1h'
            ORDER BY open_time DESC
            LIMIT 25"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .fetch_all(pool)
    .await
    .ok();
    let Some(rows) = row else { return 0.0; };
    if rows.len() < 2 {
        return 0.0;
    }
    use rust_decimal::prelude::ToPrimitive;
    let newest: f64 = rows
        .first()
        .and_then(|r| r.try_get::<rust_decimal::Decimal, _>("close").ok())
        .and_then(|d| d.to_f64())
        .unwrap_or(0.0);
    let oldest: f64 = rows
        .last()
        .and_then(|r| r.try_get::<rust_decimal::Decimal, _>("close").ok())
        .and_then(|d| d.to_f64())
        .unwrap_or(0.0);
    if oldest <= 0.0 {
        return 0.0;
    }
    (newest - oldest) / oldest
}

async fn write_event(
    pool: &PgPool,
    sym: &EngineSymbol,
    ev: &DerivEvent,
    now: DateTime<Utc>,
) -> anyhow::Result<usize> {
    let subkind = format!("{}_{}", ev.kind.as_str(), ev.variant);
    let direction: i16 = match ev.variant {
        "bull" => 1,
        "bear" => -1,
        _ => 0,
    };
    // Derivatives events attach to the symbol, not a timeframe. We
    // pick '*' as a sentinel — chart filters for exact TF match will
    // miss these, and that's the intended behaviour (the chart fetches
    // them via a separate flag in Faz 12 follow-up).
    let timeframe = "*";
    let anchors = json!([
        {
            "label_override": label_for(ev.kind),
            "time": now,
            "price": ev.metric_value,
        }
    ]);
    let raw_meta = json!({
        "score":       ev.score,
        "metric":      ev.metric_value,
        "baseline":    ev.baseline_value,
        "note":        ev.note,
        "event_kind":  ev.kind.as_str(),
    });
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
    .bind(timeframe)
    .bind(0i16)
    .bind("derivatives")
    .bind(&subkind)
    .bind(direction)
    .bind(0i64)
    .bind(0i64)
    .bind(now)
    .bind(now)
    .bind(&anchors)
    .bind(false)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

fn label_for(kind: DerivEventKind) -> &'static str {
    match kind {
        DerivEventKind::FundingSpike => "Funding",
        DerivEventKind::OiImbalance => "OI Δ",
        DerivEventKind::BasisDislocation => "Basis",
        DerivEventKind::LongShortExtreme => "LSR",
        DerivEventKind::TakerFlowImbalance => "Taker",
    }
}

// ── Config loading ─────────────────────────────────────────────────────

async fn load_num(pool: &PgPool, key: &str, field: &str, default: i64) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'derivatives' AND config_key = $1",
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
        "SELECT value FROM system_config WHERE module = 'derivatives' AND config_key = $1",
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

async fn load_config(pool: &PgPool) -> DerivConfig {
    let mut cfg = DerivConfig::default();
    cfg.min_score = load_f64(pool, "min_score", "score", cfg.min_score as f64).await as f32;
    cfg.funding_z_threshold =
        load_f64(pool, "thresholds.funding_z", "value", cfg.funding_z_threshold).await;
    cfg.funding_window =
        load_num(pool, "thresholds.funding_window", "value", cfg.funding_window as i64).await
            as usize;
    cfg.oi_delta_pct = load_f64(pool, "thresholds.oi_delta_pct", "value", cfg.oi_delta_pct).await;
    cfg.oi_price_divergence_pct = load_f64(
        pool,
        "thresholds.oi_price_divergence_pct",
        "value",
        cfg.oi_price_divergence_pct,
    )
    .await;
    cfg.basis_dislocation_pct = load_f64(
        pool,
        "thresholds.basis_dislocation_pct",
        "value",
        cfg.basis_dislocation_pct,
    )
    .await;
    cfg.lsr_long_extreme =
        load_f64(pool, "thresholds.lsr_long_extreme", "value", cfg.lsr_long_extreme).await;
    cfg.lsr_short_extreme =
        load_f64(pool, "thresholds.lsr_short_extreme", "value", cfg.lsr_short_extreme).await;
    cfg.taker_buy_dominance = load_f64(
        pool,
        "thresholds.taker_buy_dominance",
        "value",
        cfg.taker_buy_dominance,
    )
    .await;
    cfg.taker_sell_dominance = load_f64(
        pool,
        "thresholds.taker_sell_dominance",
        "value",
        cfg.taker_sell_dominance,
    )
    .await;
    cfg
}
