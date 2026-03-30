//! Tactical context bundle for prompts (FAZ 3.1 — implemented in `qtss-ai` for downstream layers).

use chrono::Utc;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sqlx::PgPool;

use qtss_storage::{
    fetch_analysis_snapshot_payload, fetch_latest_onchain_signal_score, list_enabled_engine_symbols,
    list_engine_symbols_matching, list_recent_bars, ExchangeOrderRow,
};

use crate::error::{AiError, AiResult};
use crate::storage::fetch_last_ai_decision_recent;

const BAR_LIMIT: i64 = 20;

/// Builds the JSON context described in `QTSS_MASTER_DEV_GUIDE` §3.1 (token‑budget friendly).
pub async fn build_tactical_context(pool: &PgPool, symbol: &str) -> AiResult<Value> {
    let sym = symbol.trim();
    if sym.is_empty() {
        return Err(AiError::config("empty symbol"));
    }
    let onchain = fetch_latest_onchain_signal_score(pool, sym)
        .await?
        .map(|r| serde_json::to_value(&r).unwrap_or(Value::Null));

    let engine_rows = list_engine_symbols_matching(pool, sym, None, None, None).await?;
    let engine_row = engine_rows.iter().find(|r| r.enabled).or_else(|| engine_rows.first());

    let confluence = if let Some(e) = engine_row {
        fetch_analysis_snapshot_payload(pool, e.id, "confluence")
            .await?
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };

    let price_context = if let Some(e) = engine_row {
        summarize_recent_bars(
            pool,
            &e.exchange,
            &e.segment,
            &e.symbol,
            &e.interval,
            BAR_LIMIT,
        )
        .await?
    } else {
        Value::Null
    };

    let open_position = recent_submitted_order_summary(pool, sym).await?;

    let last_ai_decision = fetch_last_ai_decision_recent(pool, sym, 24)
        .await?
        .map(|r| {
            json!({
                "id": r.id,
                "created_at": r.created_at,
                "parsed_decision": r.parsed_decision,
                "confidence": r.confidence,
                "status": r.status,
            })
        })
        .unwrap_or(Value::Null);

    Ok(json!({
        "symbol": sym.to_uppercase(),
        "timestamp_utc": Utc::now(),
        "onchain_signals": onchain,
        "confluence": confluence,
        "price_context": price_context,
        "open_position": open_position,
        "last_ai_decision": last_ai_decision,
    }))
}

pub(super) async fn summarize_recent_bars(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    interval: &str,
    bar_limit: i64,
) -> AiResult<Value> {
    let bars = list_recent_bars(pool, exchange, segment, symbol, interval, bar_limit).await?;
    if bars.is_empty() {
        return Ok(Value::Null);
    }
    let mut highs = Vec::new();
    let mut lows = Vec::new();
    let mut closes = Vec::new();
    for b in &bars {
        highs.push(b.high.to_f64().unwrap_or(0.0));
        lows.push(b.low.to_f64().unwrap_or(0.0));
        closes.push(b.close.to_f64().unwrap_or(0.0));
    }
    let last_close = closes.first().copied().unwrap_or(0.0);
    let oldest_close = closes.last().copied().unwrap_or(last_close);
    let pct_change_24h_hint = if oldest_close.abs() > f64::EPSILON {
        ((last_close - oldest_close) / oldest_close) * 100.0
    } else {
        0.0
    };
    let high_max = highs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let low_min = lows.iter().copied().fold(f64::INFINITY, f64::min);
    let mean_close = closes.iter().sum::<f64>() / closes.len().max(1) as f64;
    let range_vs_mean_pct = if mean_close.abs() > f64::EPSILON {
        ((high_max - low_min) / mean_close) * 100.0
    } else {
        0.0
    };
    Ok(json!({
        "exchange": exchange,
        "segment": segment,
        "interval": interval,
        "bars_used": bars.len(),
        "last_close": last_close,
        "approx_change_over_window_pct": pct_change_24h_hint,
        "high_low_range_pct_of_mean_close": range_vs_mean_pct,
        "last_bar_open_time": bars.first().map(|b| b.open_time),
    }))
}

async fn recent_submitted_order_summary(pool: &PgPool, symbol: &str) -> AiResult<Value> {
    let sym = symbol.trim().to_uppercase();
    let rows: Vec<ExchangeOrderRow> = sqlx::query_as(
        r#"SELECT id, org_id, user_id, exchange, segment, symbol,
                  client_order_id, status, intent, venue_order_id,
                  venue_response, created_at, updated_at
           FROM exchange_orders
           WHERE symbol = $1 AND status = 'submitted'
           ORDER BY updated_at DESC
           LIMIT 5"#,
    )
    .bind(&sym)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(Value::Null);
    }
    let brief: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "exchange": r.exchange,
                "segment": r.segment,
                "status": r.status,
                "intent_side": r.intent.get("side"),
                "intent_qty": r.intent.get("qty"),
                "updated_at": r.updated_at,
            })
        })
        .collect();
    Ok(json!({ "recent_submitted": brief }))
}

const OP_BARS: i64 = 5;

/// Operational context (~1000 token target): position leg uses symbol-level fills summary only here.
pub async fn build_operational_context(pool: &PgPool, symbol: &str) -> AiResult<Value> {
    let sym = symbol.trim();
    if sym.is_empty() {
        return Err(AiError::config("empty symbol"));
    }
    let onchain = fetch_latest_onchain_signal_score(pool, sym)
        .await?
        .map(|r| serde_json::to_value(&r).unwrap_or(Value::Null));
    let engine_rows = list_engine_symbols_matching(pool, sym, None, None, None).await?;
    let engine_row = engine_rows.iter().find(|r| r.enabled).or_else(|| engine_rows.first());
    let recent_price_stats = if let Some(e) = engine_row {
        summarize_recent_bars(pool, &e.exchange, &e.segment, &e.symbol, &e.interval, OP_BARS).await?
    } else {
        Value::Null
    };
    Ok(json!({
        "symbol": sym.to_uppercase(),
        "timestamp_utc": Utc::now(),
        "onchain_signals": onchain,
        "recent_price_stats": recent_price_stats,
        "funding_snapshot": Value::Null,
    }))
}

/// Strategic context: enabled symbols confluence headlines + `pnl_rollups` 7d sample + outcome stats.
pub async fn build_strategic_context(pool: &PgPool) -> AiResult<Value> {
    let engine = list_enabled_engine_symbols(pool).await?;
    let mut symbol_confluence: Vec<Value> = Vec::new();
    for e in engine.iter().take(80) {
        let payload = fetch_analysis_snapshot_payload(pool, e.id, "confluence")
            .await?
            .unwrap_or(Value::Null);
        let brief = json!({
            "symbol": e.symbol,
            "exchange": e.exchange,
            "segment": e.segment,
            "confluence_payload_excerpt": payload,
        });
        symbol_confluence.push(brief);
    }

    let pnl_rows: Vec<(String, Option<String>, f64, f64, i64)> =
        match sqlx::query_as::<_, (String, Option<String>, f64, f64, i64)>(
            r#"SELECT ledger, symbol, COALESCE(realized_pnl, 0)::float8, COALESCE(volume, 0)::float8, trade_count
           FROM pnl_rollups
           WHERE period_start >= now() - interval '7 days'
             AND bucket = 'daily'
           ORDER BY period_start DESC
           LIMIT 200"#,
        )
        .fetch_all(pool)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(%e, "strategic context: pnl_rollups read failed");
                vec![]
            }
        };

    let pnl_summary = json!({
        "rows_sampled": pnl_rows.len(),
        "rows": pnl_rows.into_iter().take(40).collect::<Vec<_>>(),
    });

    let feedback = crate::storage::fetch_recent_outcome_stats(pool, 30).await?;

    Ok(json!({
        "timestamp_utc": Utc::now(),
        "enabled_engine_symbols": engine.len(),
        "symbol_confluence": symbol_confluence,
        "pnl_rollups_7d_hint": pnl_summary,
        "recent_ai_outcomes": feedback,
    }))
}
