//! Faz 15 — Order-flow deep detectors.
//!
//! Builds on Faz 12C basics (LiquidationCluster / BlockTrade / CVD
//! Divergence) with three richer signal families:
//!
//!   * **FootprintImbalance** — per-bar taker-buy vs taker-sell ratio
//!     extremes inside the 1-minute CVD buckets. Sniffs out bars
//!     where flow is lopsided enough to imply directional intent
//!     (trend ignition, capitulation).
//!   * **Absorption** — cumulative delta prints large but price
//!     barely moves (support / resistance is absorbing the flow).
//!     Classic hidden buy-at-low or sell-at-high tape.
//!   * **Sweep** — rapid successive-level taking: three+ consecutive
//!     buckets with taker-buy (or taker-sell) > X% and cumulative
//!     delta climbing. Often precedes strong breakouts.
//!
//! Same contract as Faz 12C: pure functions over JSON payload + config,
//! returns `Vec<OrderFlowEvent>`. The engine writer just needs new
//! dispatch entries to persist the new kinds.

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

/// Minimum per-bucket taker-buy share (fraction 0..1) to qualify a
/// bucket as buyer-dominated. Flip for sell-dominated with 1 - this.
const BUYER_DOMINANCE_THRESHOLD: f64 = 0.65;

/// Sweep length — consecutive same-side dominated buckets.
const SWEEP_MIN_RUN: usize = 3;

// ── FootprintImbalance — per-bucket taker ratio extremes ─────────────

pub fn detect_footprint_imbalance(
    payload: &Value,
    cfg: &OrderFlowConfig,
) -> Vec<OrderFlowEvent> {
    let mut out = Vec::new();
    let buckets = payload.get("buckets").and_then(|v| v.as_array());
    let Some(buckets) = buckets else { return out; };
    for b in buckets.iter().rev().take(6) {
        let buy = b.get("buy_qty").and_then(as_f64).unwrap_or(0.0);
        let sell = b.get("sell_qty").and_then(as_f64).unwrap_or(0.0);
        let total = buy + sell;
        if total < 1e-9 {
            continue;
        }
        let buy_share = buy / total;
        let ts = b.get("bucket_ts_ms").and_then(as_i64).unwrap_or(0);
        let price = 0.0; // bucket doesn't carry price — caller sets.
        if buy_share >= BUYER_DOMINANCE_THRESHOLD {
            let strength = (buy_share - 0.5) * 2.0; // 0..1
            out.push(OrderFlowEvent {
                kind: OrderFlowEventKind::LiquidationCluster, // reused slot
                variant: "bull",
                score: strength.max(0.3),
                magnitude: total,
                reference_price: price,
                event_time_ms: ts,
                note: format!(
                    "Footprint alım ağırlıklı: {:.0}% alıcı ({:.2} qty)",
                    buy_share * 100.0,
                    total
                ),
            });
        } else if (1.0 - buy_share) >= BUYER_DOMINANCE_THRESHOLD {
            let strength = (0.5 - buy_share) * 2.0;
            out.push(OrderFlowEvent {
                kind: OrderFlowEventKind::LiquidationCluster,
                variant: "bear",
                score: strength.max(0.3),
                magnitude: total,
                reference_price: price,
                event_time_ms: ts,
                note: format!(
                    "Footprint satış ağırlıklı: {:.0}% satıcı ({:.2} qty)",
                    (1.0 - buy_share) * 100.0,
                    total
                ),
            });
        }
    }
    // Cap to min_score after the caller inspects them — the writer
    // handles the gate. This lets operators tune `min_score` without
    // re-deploying the crate.
    let _ = cfg;
    out
}

// ── Absorption — delta prints large, price barely moves ─────────────

pub fn detect_absorption(
    payload: &Value,
    bar_closes: &[f64],
    cfg: &OrderFlowConfig,
) -> Vec<OrderFlowEvent> {
    let mut out = Vec::new();
    let buckets = payload.get("buckets").and_then(|v| v.as_array());
    let Some(buckets) = buckets else { return out; };
    if buckets.is_empty() || bar_closes.len() < 2 {
        return out;
    }
    // Sum the last N bucket deltas; compare to short-range price move.
    let n = 10.min(buckets.len());
    let tail: Vec<&Value> = buckets.iter().rev().take(n).collect();
    let abs_delta_sum: f64 = tail
        .iter()
        .filter_map(|b| b.get("delta").and_then(as_f64))
        .map(|d| d.abs())
        .sum();
    if abs_delta_sum < 1e-9 {
        return out;
    }
    let price_last = *bar_closes.last().unwrap();
    let price_lookback = bar_closes[bar_closes.len().saturating_sub(5)];
    if price_last == 0.0 {
        return out;
    }
    let price_change_pct = (price_last - price_lookback).abs() / price_last;
    // If price drifted less than 0.25% but cumulative delta moved a
    // large chunk, the flow is being absorbed at a level.
    if price_change_pct > 0.0025 {
        return out;
    }
    let sum_delta: f64 = tail
        .iter()
        .filter_map(|b| b.get("delta").and_then(as_f64))
        .sum();
    // Signed dominance — positive = absorption by sellers (bulls
    // pressing but price held → bear bias); negative = absorption by
    // buyers (bears pressing but price held → bull bias).
    let (variant, note): (&'static str, String) = if sum_delta > 0.0 {
        (
            "bear",
            format!(
                "Absorption: Δ=+{:.0} toplam, fiyat hareketsiz — alıcı baskısı emiliyor",
                sum_delta
            ),
        )
    } else {
        (
            "bull",
            format!(
                "Absorption: Δ={:.0} toplam, fiyat hareketsiz — satıcı baskısı emiliyor",
                sum_delta
            ),
        )
    };
    let ts = tail
        .first()
        .and_then(|b| b.get("bucket_ts_ms").and_then(as_i64))
        .unwrap_or(0);
    let score = (abs_delta_sum / 100.0).min(1.0).max(cfg.min_score as f64);
    out.push(OrderFlowEvent {
        kind: OrderFlowEventKind::CvdDivergence, // re-using slot
        variant,
        score,
        magnitude: sum_delta,
        reference_price: price_last,
        event_time_ms: ts,
        note,
    });
    out
}

// ── Sweep — N consecutive same-side dominant buckets ────────────────

pub fn detect_sweep(payload: &Value, cfg: &OrderFlowConfig) -> Vec<OrderFlowEvent> {
    let mut out = Vec::new();
    let buckets = payload.get("buckets").and_then(|v| v.as_array());
    let Some(buckets) = buckets else { return out; };
    if buckets.len() < SWEEP_MIN_RUN {
        return out;
    }
    // Walk from newest backward, counting same-side dominant run.
    let mut run_side: Option<&'static str> = None;
    let mut run_len = 0usize;
    let mut run_vol = 0.0f64;
    let mut run_end_ts = 0i64;
    for b in buckets.iter().rev() {
        let buy = b.get("buy_qty").and_then(as_f64).unwrap_or(0.0);
        let sell = b.get("sell_qty").and_then(as_f64).unwrap_or(0.0);
        let total = buy + sell;
        if total < 1e-9 {
            break;
        }
        let buy_share = buy / total;
        let side = if buy_share >= BUYER_DOMINANCE_THRESHOLD {
            Some("bull")
        } else if (1.0 - buy_share) >= BUYER_DOMINANCE_THRESHOLD {
            Some("bear")
        } else {
            None
        };
        match (run_side, side) {
            (None, Some(s)) => {
                run_side = Some(s);
                run_len = 1;
                run_vol = total;
                run_end_ts = b
                    .get("bucket_ts_ms")
                    .and_then(as_i64)
                    .unwrap_or(0);
            }
            (Some(existing), Some(s)) if existing == s => {
                run_len += 1;
                run_vol += total;
            }
            _ => break,
        }
    }
    if run_len >= SWEEP_MIN_RUN {
        let variant = run_side.unwrap_or("bull");
        let score = (run_len as f64 / 6.0).min(1.0).max(cfg.min_score as f64);
        let note = format!(
            "Sweep: {} consecutive {}-dominated buckets ({:.0} qty)",
            run_len, variant, run_vol
        );
        out.push(OrderFlowEvent {
            kind: OrderFlowEventKind::LiquidationCluster, // reusing slot
            variant,
            score,
            magnitude: run_vol,
            reference_price: 0.0,
            event_time_ms: run_end_ts,
            note,
        });
    }
    out
}
