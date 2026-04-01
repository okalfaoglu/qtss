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
use crate::storage::{fetch_active_portfolio_directive, fetch_last_ai_decision_recent, fetch_symbol_outcome_stats};

const BAR_LIMIT: i64 = 20;

/// Approximate max chars for context JSON (~token budget × 4 chars/token).
/// Exceeding this triggers truncation of low-priority fields.
const MAX_CONTEXT_CHARS: usize = 12_000; // ~3000 tokens

/// Trim context JSON to stay within token budget by removing low-priority fields.
fn trim_context_if_needed(ctx: &mut Value) {
    let serialized_len = serde_json::to_string(ctx).map(|s| s.len()).unwrap_or(0);
    if serialized_len <= MAX_CONTEXT_CHARS {
        return;
    }
    // Priority order for removal: ai_feedback → confluence → open_position → onchain_signals
    let low_priority_keys = ["ai_feedback", "confluence", "open_position"];
    if let Some(obj) = ctx.as_object_mut() {
        for key in &low_priority_keys {
            obj.remove(*key);
            let new_len = serde_json::to_string(&Value::Object(obj.clone()))
                .map(|s| s.len())
                .unwrap_or(0);
            if new_len <= MAX_CONTEXT_CHARS {
                obj.insert("_trimmed".into(), json!(key));
                return;
            }
        }
        obj.insert("_trimmed".into(), json!("multiple_fields"));
    }
}

/// OHLC window metrics for prompts (`closes[0]` = newest bar, `closes[last]` = oldest in window).
/// Extracted for unit tests (FAZ 3 / `QTSS_MASTER_DEV_GUIDE` §7).
pub(crate) fn bar_ohlc_window_metrics(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
) -> Option<(f64, f64, f64, usize)> {
    if closes.is_empty() || highs.len() != closes.len() || lows.len() != closes.len() {
        return None;
    }
    let last_close = closes.first().copied().unwrap_or(0.0);
    let oldest_close = closes.last().copied().unwrap_or(last_close);
    let pct_change = if oldest_close.abs() > f64::EPSILON {
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
    Some((last_close, pct_change, range_vs_mean_pct, closes.len()))
}

fn portfolio_symbol_weight(symbol_scores: &Value, symbol: &str) -> Option<f64> {
    let o = symbol_scores.as_object()?;
    let needle = symbol.trim().to_uppercase();
    for (k, v) in o {
        if k.trim().to_uppercase() == needle {
            return v
                .as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()));
        }
    }
    None
}

fn portfolio_directive_summary_for_symbol(
    row: &crate::storage::AiPortfolioDirectiveRow,
    symbol: &str,
) -> Value {
    let weight = portfolio_symbol_weight(&row.symbol_scores, symbol);
    json!({
        "risk_budget_pct": row.risk_budget_pct,
        "max_open_positions": row.max_open_positions,
        "preferred_regime": row.preferred_regime,
        "macro_note": row.macro_note,
        "symbol_weight_0_1": weight,
        "valid_until": row.valid_until,
    })
}

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

    let ai_feedback = fetch_symbol_outcome_stats(pool, sym, 20).await.unwrap_or(Value::Null);

    let portfolio_directive = match fetch_active_portfolio_directive(pool).await? {
        Some(row) => {
            let stale = row
                .valid_until
                .is_some_and(|u| u < Utc::now());
            if stale {
                Value::Null
            } else {
                portfolio_directive_summary_for_symbol(&row, sym)
            }
        }
        None => Value::Null,
    };

    let mut ctx = json!({
        "symbol": sym.to_uppercase(),
        "timestamp_utc": Utc::now(),
        "onchain_signals": onchain,
        "confluence": confluence,
        "price_context": price_context,
        "open_position": open_position,
        "last_ai_decision": last_ai_decision,
        "ai_feedback": ai_feedback,
        "portfolio_directive": portfolio_directive,
    });
    trim_context_if_needed(&mut ctx);
    Ok(ctx)
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
    let (last_close, pct_change_24h_hint, range_vs_mean_pct, bars_used) =
        bar_ohlc_window_metrics(&highs, &lows, &closes).unwrap_or((0.0, 0.0, 0.0, 0));
    Ok(json!({
        "exchange": exchange,
        "segment": segment,
        "interval": interval,
        "bars_used": bars_used,
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
    let ai_feedback = fetch_symbol_outcome_stats(pool, sym, 20).await.unwrap_or(Value::Null);
    Ok(json!({
        "symbol": sym.to_uppercase(),
        "timestamp_utc": Utc::now(),
        "onchain_signals": onchain,
        "recent_price_stats": recent_price_stats,
        "ai_feedback": ai_feedback,
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

    let mut ctx = json!({
        "timestamp_utc": Utc::now(),
        "enabled_engine_symbols": engine.len(),
        "symbol_confluence": symbol_confluence,
        "pnl_rollups_7d_hint": pnl_summary,
        "recent_ai_outcomes": feedback,
    });
    // Strategic context has a larger budget (4096 output tokens, ~50k chars input budget).
    const MAX_STRATEGIC_CHARS: usize = 50_000;
    let len = serde_json::to_string(&ctx).map(|s| s.len()).unwrap_or(0);
    if len > MAX_STRATEGIC_CHARS {
        // Truncate symbol_confluence array until it fits
        loop {
            let cur = serde_json::to_string(&ctx).map(|s| s.len()).unwrap_or(0);
            if cur <= MAX_STRATEGIC_CHARS {
                break;
            }
            let should_pop = ctx
                .get("symbol_confluence")
                .and_then(|v| v.as_array())
                .map(|a| a.len() > 5)
                .unwrap_or(false);
            if !should_pop {
                break;
            }
            if let Some(arr) = ctx.get_mut("symbol_confluence").and_then(|v| v.as_array_mut()) {
                arr.pop();
            }
        }
    }
    Ok(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_metrics_empty_none() {
        assert!(bar_ohlc_window_metrics(&[], &[], &[]).is_none());
    }

    #[test]
    fn bar_metrics_mismatched_lengths_none() {
        assert!(bar_ohlc_window_metrics(&[1.0], &[1.0, 2.0], &[1.0]).is_none());
    }

    #[test]
    fn bar_metrics_ten_percent_up_over_window() {
        // newest first: 110, oldest 100 → +10%
        let highs = vec![111.0, 100.0];
        let lows = vec![109.0, 99.0];
        let closes = vec![110.0, 100.0];
        let (last, pct, _, n) = bar_ohlc_window_metrics(&highs, &lows, &closes).unwrap();
        assert!((last - 110.0).abs() < f64::EPSILON);
        assert!((pct - 10.0).abs() < 1e-9);
        assert_eq!(n, 2);
    }

    #[test]
    fn trim_context_removes_low_priority_fields() {
        // Build a context that exceeds MAX_CONTEXT_CHARS
        let big_str = "x".repeat(MAX_CONTEXT_CHARS + 1000);
        let mut ctx = json!({
            "symbol": "BTCUSDT",
            "confluence": big_str,
            "ai_feedback": {"win_rate": 0.5},
            "price_context": {"last_close": 100.0},
        });
        trim_context_if_needed(&mut ctx);
        // ai_feedback should be removed first (lowest priority)
        assert!(ctx.get("ai_feedback").is_none());
        assert!(ctx.get("_trimmed").is_some());
    }

    #[test]
    fn trim_context_noop_when_small() {
        let mut ctx = json!({
            "symbol": "BTCUSDT",
            "confluence": "small",
            "ai_feedback": {"win_rate": 0.5},
        });
        trim_context_if_needed(&mut ctx);
        assert!(ctx.get("ai_feedback").is_some());
        assert!(ctx.get("_trimmed").is_none());
    }

    #[test]
    fn portfolio_symbol_weight_case_insensitive() {
        let scores = json!({ "BTCUSDT": 0.8, "ethusdt": "0.5" });
        assert!((portfolio_symbol_weight(&scores, "btcusdt").unwrap() - 0.8).abs() < f64::EPSILON);
        assert!((portfolio_symbol_weight(&scores, "ETHUSDT").unwrap() - 0.5).abs() < f64::EPSILON);
        assert!(portfolio_symbol_weight(&scores, "SOLUSDT").is_none());
    }
}
