//! Order-flow event evaluators. Each takes a JSON payload and a
//! config, returns Vec<OrderFlowEvent>. Pure functions — no DB, no
//! clock. Engine writer wraps these in a transaction + upsert.

use crate::config::OrderFlowConfig;
use crate::event::{OrderFlowEvent, OrderFlowEventKind};
use serde_json::Value;

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    }
}

// ── LiquidationCluster ────────────────────────────────────────────────

pub fn detect_liquidation_cluster(
    payload: &Value,
    cfg: &OrderFlowConfig,
) -> Vec<OrderFlowEvent> {
    let mut out = Vec::new();
    let events = payload.get("events").and_then(|v| v.as_array());
    let Some(events) = events else { return out; };
    if events.is_empty() {
        return out;
    }
    // Window the events by the most-recent timestamp minus window_secs.
    let now_ms = events
        .iter()
        .filter_map(|e| e.get("ts_ms").and_then(as_i64))
        .max()
        .unwrap_or(0);
    let cutoff_ms = now_ms - cfg.liq_cluster_window_secs * 1000;

    let mut buy_count = 0usize;
    let mut sell_count = 0usize;
    let mut buy_notional = 0.0f64;
    let mut sell_notional = 0.0f64;
    let mut last_price = 0.0f64;
    for e in events {
        let Some(ts) = e.get("ts_ms").and_then(as_i64) else { continue };
        if ts < cutoff_ms {
            continue;
        }
        let price = e.get("price").and_then(as_f64).unwrap_or(0.0);
        let qty = e.get("qty").and_then(as_f64).unwrap_or(0.0);
        let notional = price * qty;
        let side = e.get("side").and_then(|s| s.as_str()).unwrap_or("");
        last_price = price;
        if side == "BUY" {
            // Short liquidations (buy-to-cover) → bullish squeeze marker.
            buy_count += 1;
            buy_notional += notional;
        } else if side == "SELL" {
            sell_count += 1;
            sell_notional += notional;
        }
    }
    let total_count = buy_count + sell_count;
    let total_notional = buy_notional + sell_notional;
    if total_count < cfg.liq_cluster_min_count && total_notional < cfg.liq_cluster_min_notional_usd
    {
        return out;
    }
    // Dominant side picks the variant. Short-side liqs = price ripping
    // up (bullish squeeze); long-side = price flushing (bearish capit).
    let (variant, note): (&'static str, String) = if buy_notional > sell_notional * 1.5 {
        (
            "bull",
            format!(
                "{buy_count} short-liq ({}k$) — short squeeze",
                (buy_notional / 1_000.0).round()
            ),
        )
    } else if sell_notional > buy_notional * 1.5 {
        (
            "bear",
            format!(
                "{sell_count} long-liq ({}k$) — long kapitülasyon",
                (sell_notional / 1_000.0).round()
            ),
        )
    } else {
        (
            "neutral",
            format!(
                "{total_count} liq ({}k$) iki yönlü kaos",
                (total_notional / 1_000.0).round()
            ),
        )
    };
    let notional_score =
        (total_notional / (cfg.liq_cluster_min_notional_usd * 3.0)).min(1.0) * 0.6;
    let count_score = (total_count as f64 / (cfg.liq_cluster_min_count as f64 * 3.0)).min(1.0) * 0.4;
    out.push(OrderFlowEvent {
        kind: OrderFlowEventKind::LiquidationCluster,
        variant,
        score: (notional_score + count_score).clamp(0.0, 1.0),
        magnitude: total_notional,
        reference_price: last_price,
        event_time_ms: now_ms,
        note,
    });
    out
}

// ── BlockTrade ────────────────────────────────────────────────────────

pub fn detect_block_trades(payload: &Value, cfg: &OrderFlowConfig) -> Vec<OrderFlowEvent> {
    let mut out = Vec::new();
    let events = payload.get("events").and_then(|v| v.as_array());
    let Some(events) = events else { return out; };
    for e in events {
        let price = e.get("price").and_then(as_f64).unwrap_or(0.0);
        let qty = e.get("qty").and_then(as_f64).unwrap_or(0.0);
        let notional = price * qty;
        if notional < cfg.block_trade_notional_usd {
            continue;
        }
        let side = e.get("side").and_then(|s| s.as_str()).unwrap_or("");
        let ts = e.get("ts_ms").and_then(as_i64).unwrap_or(0);
        let (variant, note): (&'static str, String) = if side == "BUY" {
            (
                "bull",
                format!("Blok kısa likidasyonu: {}k$ alış baskısı", (notional / 1_000.0).round()),
            )
        } else {
            (
                "bear",
                format!("Blok uzun likidasyonu: {}k$ satış baskısı", (notional / 1_000.0).round()),
            )
        };
        // Score scales with size over threshold, capped at 5×.
        let over = (notional / cfg.block_trade_notional_usd).min(5.0);
        let score = (over - 1.0).max(0.0) / 4.0 * 0.7 + 0.3;
        out.push(OrderFlowEvent {
            kind: OrderFlowEventKind::BlockTrade,
            variant,
            score,
            magnitude: notional,
            reference_price: price,
            event_time_ms: ts,
            note,
        });
    }
    out
}

// ── CVDDivergence ─────────────────────────────────────────────────────

pub fn detect_cvd_divergence(
    payload: &Value,
    bar_closes: &[f64],
    cfg: &OrderFlowConfig,
) -> Vec<OrderFlowEvent> {
    let mut out = Vec::new();
    let buckets = payload.get("buckets").and_then(|v| v.as_array());
    let Some(buckets) = buckets else { return out; };
    if buckets.len() < cfg.cvd_divergence_bars || bar_closes.len() < cfg.cvd_divergence_bars {
        return out;
    }
    let tail_cvd: Vec<f64> = buckets[buckets.len() - cfg.cvd_divergence_bars..]
        .iter()
        .filter_map(|b| b.get("cvd").and_then(as_f64))
        .collect();
    if tail_cvd.len() < cfg.cvd_divergence_bars {
        return out;
    }
    let tail_price = &bar_closes[bar_closes.len() - cfg.cvd_divergence_bars..];
    let price_first = tail_price[0];
    let price_last = tail_price[tail_price.len() - 1];
    if price_first <= 0.0 {
        return out;
    }
    let price_change_pct = (price_last - price_first) / price_first;
    if price_change_pct.abs() < cfg.cvd_divergence_price_min_pct {
        return out;
    }
    let cvd_first = tail_cvd[0];
    let cvd_last = tail_cvd[tail_cvd.len() - 1];
    let cvd_abs_sum: f64 = tail_cvd
        .iter()
        .zip(tail_cvd.iter().skip(1))
        .map(|(a, b)| (b - a).abs())
        .sum();
    if cvd_abs_sum <= 0.0 {
        return out;
    }
    let cvd_change = cvd_last - cvd_first;
    let cvd_rel = cvd_change / cvd_abs_sum.abs().max(1e-9);
    // Bearish divergence: price up but CVD flat/down.
    let bearish = price_change_pct > 0.0 && cvd_rel <= -cfg.cvd_divergence_cvd_opposite_min;
    let bullish = price_change_pct < 0.0 && cvd_rel >= cfg.cvd_divergence_cvd_opposite_min;
    if !bearish && !bullish {
        return out;
    }
    let (variant, note): (&'static str, String) = if bearish {
        (
            "bear",
            format!(
                "Fiyat %{:.1} yükseldi, CVD ters yönlü — alış desteği yok",
                price_change_pct * 100.0
            ),
        )
    } else {
        (
            "bull",
            format!(
                "Fiyat %{:.1} düştü, CVD ters yönlü — satış desteği yok",
                price_change_pct * 100.0
            ),
        )
    };
    let price_strength = (price_change_pct.abs() / cfg.cvd_divergence_price_min_pct).min(3.0) / 3.0;
    let cvd_strength = (cvd_rel.abs() / cfg.cvd_divergence_cvd_opposite_min).min(3.0) / 3.0;
    let score = (price_strength * 0.4 + cvd_strength * 0.6).clamp(0.0, 1.0);
    out.push(OrderFlowEvent {
        kind: OrderFlowEventKind::CvdDivergence,
        variant,
        score,
        magnitude: cvd_rel,
        reference_price: price_last,
        event_time_ms: 0,
        note,
    });
    out
}
