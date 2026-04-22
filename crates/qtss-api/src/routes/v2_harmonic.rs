//! `GET /v2/harmonic/{venue}/{symbol}/{tf}` — live-computed harmonic
//! patterns. Mirrors the `/v2/harmonic-db` shape so the chart can
//! swap sources with a URL change; differs only in that pivots come
//! straight from `compute_pivots` on fresh bars, no DB read.
//!
//! The matching logic is deliberately duplicated with
//! `crates/qtss-worker/src/harmonic_writer_loop.rs` — both paths
//! consume [`qtss_harmonic::PATTERNS`] directly so they can't drift
//! on ratio thresholds. Factoring into a shared helper would add a
//! new crate boundary for 30 lines; the duplication is cheap and
//! audited by both endpoints having identical pattern output when
//! pointed at the same bar set.
//!
//! Default Z-slots + lengths (3,5,8,13,21) stay in sync with
//! `/v2/zigzag` and `/v2/elliott` so a pivot emitted at Z3 is the
//! same pivot across every endpoint.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::Row;

use qtss_harmonic::{match_pattern, HarmonicSpec, XabcdPoints, PATTERNS};
use qtss_pivots::zigzag::{compute_pivots, Sample};
use qtss_storage::{market_bars, market_bars_open};

use crate::error::ApiError;
use crate::state::SharedState;

use super::v2_harmonic_db::{HarmonicAnchor, HarmonicCandle, HarmonicPattern, HarmonicResponse};

#[derive(Debug, Deserialize)]
pub struct HarmonicQuery {
    pub limit: Option<i64>,
    pub segment: Option<String>,
    pub slot: Option<i16>,
    pub subkind: Option<String>,
}

pub fn v2_harmonic_router() -> Router<SharedState> {
    Router::new().route("/v2/harmonic/{venue}/{symbol}/{tf}", get(get_harmonic))
}

/// Read the same Z-slot lengths the /v2/zigzag endpoint uses so all
/// three live paths (zigzag / elliott / harmonic) agree on which pivots
/// exist. Operator tuning via `system_config.zigzag.slot_N.length`
/// propagates here without extra keys.
async fn load_slot_lengths(pool: &sqlx::PgPool) -> [u32; 5] {
    let defaults: [u32; 5] = [3, 5, 8, 13, 21];
    let mut out = defaults;
    for i in 0..5usize {
        let key = format!("slot_{i}");
        if let Ok(Some(row)) = sqlx::query(
            "SELECT value FROM system_config WHERE module = 'zigzag' AND config_key = $1",
        )
        .bind(&key)
        .fetch_optional(pool)
        .await
        {
            let val: serde_json::Value = row.try_get("value").unwrap_or(serde_json::Value::Null);
            if let Some(len) = val.get("length").and_then(|v| v.as_u64()) {
                out[i] = (len.max(1)) as u32;
            }
        }
    }
    out
}

async fn load_min_score(pool: &sqlx::PgPool) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'harmonic' AND config_key = 'min_score'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.60; };
    let val: serde_json::Value = row.try_get("value").unwrap_or(serde_json::Value::Null);
    val.get("score").and_then(|v| v.as_f64()).unwrap_or(0.60).clamp(0.0, 1.0)
}

async fn load_slack(pool: &sqlx::PgPool) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'harmonic' AND config_key = 'slack'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.05; };
    let val: serde_json::Value = row.try_get("value").unwrap_or(serde_json::Value::Null);
    val.get("slack").and_then(|v| v.as_f64()).unwrap_or(0.05).clamp(0.0, 0.2)
}

async fn get_harmonic(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<HarmonicQuery>,
) -> Result<Json<HarmonicResponse>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(1000).clamp(1, 5_000);
    let min_score = load_min_score(&st.pool).await;
    let slack = load_slack(&st.pool).await;
    let lengths = load_slot_lengths(&st.pool).await;

    // Candles — same loader convention as zigzag/elliott/harmonic-db.
    let rows =
        market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, limit).await?;
    let mut candles: Vec<HarmonicCandle> = rows
        .into_iter()
        .rev()
        .enumerate()
        .map(|(i, r)| HarmonicCandle {
            time: r.open_time,
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume,
            bar_index: i as i64,
        })
        .collect();

    // Merge live in-progress bar (same convention as /v2/zigzag).
    if let Ok(Some(open_bar)) =
        market_bars_open::get_open_bar(&st.pool, &venue, &segment, &symbol, &tf).await
    {
        let is_newer = candles
            .last()
            .map(|c| open_bar.open_time > c.time)
            .unwrap_or(true);
        if is_newer {
            let next_idx = candles.len() as i64;
            candles.push(HarmonicCandle {
                time: open_bar.open_time,
                open: open_bar.open,
                high: open_bar.high,
                low: open_bar.low,
                close: open_bar.close,
                volume: open_bar.volume,
                bar_index: next_idx,
            });
        }
    }

    let samples: Vec<Sample> = candles
        .iter()
        .map(|c| Sample {
            bar_index: c.bar_index as u64,
            time: c.time,
            high: c.high,
            low: c.low,
            volume: c.volume,
        })
        .collect();

    let mut patterns: Vec<HarmonicPattern> = Vec::new();
    for (slot_idx, &length) in lengths.iter().enumerate() {
        if let Some(filter_slot) = q.slot {
            if (slot_idx as i16) != filter_slot {
                continue;
            }
        }
        let pivots = compute_pivots(&samples, length);
        if pivots.len() < 5 {
            continue;
        }
        for start in 0..=(pivots.len() - 5) {
            let window = &pivots[start..start + 5];
            // Strict alternation (H/L/H/L/H or L/H/L/H/L).
            if !alternation_ok(window) {
                continue;
            }
            let bullish = matches!(window[0].kind, qtss_domain::v2::pivot::PivotKind::Low);
            let pts = make_xabcd(window, bullish);
            let Some((spec, score)) = best_pattern(&pts, slack) else { continue };
            if score < min_score {
                continue;
            }
            let subkind = format!(
                "{}_{}",
                spec.name,
                if bullish { "bull" } else { "bear" }
            );
            if let Some(ref want) = q.subkind {
                if want != &subkind {
                    continue;
                }
            }
            let direction: i16 = if bullish { 1 } else { -1 };
            patterns.push(build_pattern(
                slot_idx as i16,
                subkind,
                direction,
                window,
                score,
                &pts,
                spec,
            ));
        }
    }

    Ok(Json(HarmonicResponse {
        venue,
        symbol,
        timeframe: tf,
        candles,
        patterns,
    }))
}

fn alternation_ok(window: &[qtss_pivots::zigzag::ConfirmedPivot]) -> bool {
    window
        .windows(2)
        .all(|w| !matches!((&w[0].kind, &w[1].kind),
            (qtss_domain::v2::pivot::PivotKind::High, qtss_domain::v2::pivot::PivotKind::High) |
            (qtss_domain::v2::pivot::PivotKind::Low, qtss_domain::v2::pivot::PivotKind::Low)))
}

fn make_xabcd(window: &[qtss_pivots::zigzag::ConfirmedPivot], bullish: bool) -> XabcdPoints {
    let p = |i: usize| -> f64 {
        let v = window[i].price.to_f64().unwrap_or(0.0);
        if bullish { v } else { -v }
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
) -> Option<(&'static HarmonicSpec, f64)> {
    let mut best_spec: Option<&'static HarmonicSpec> = None;
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

fn build_pattern(
    slot: i16,
    subkind: String,
    direction: i16,
    window: &[qtss_pivots::zigzag::ConfirmedPivot],
    score: f64,
    pts: &XabcdPoints,
    spec: &HarmonicSpec,
) -> HarmonicPattern {
    const LABELS: [&str; 5] = ["X", "A", "B", "C", "D"];
    let anchors: Vec<HarmonicAnchor> = window
        .iter()
        .enumerate()
        .map(|(i, p)| HarmonicAnchor {
            bar_index: p.bar_index as i64,
            time: p.time,
            price: p.price.to_f64().unwrap_or(0.0),
            label: LABELS[i].to_string(),
        })
        .collect();
    let start_time: DateTime<Utc> = anchors.first().map(|a| a.time).unwrap_or_else(Utc::now);
    let end_time: DateTime<Utc> = anchors.last().map(|a| a.time).unwrap_or_else(Utc::now);
    let start_bar = anchors.first().map(|a| a.bar_index).unwrap_or(0);
    let end_bar = anchors.last().map(|a| a.bar_index).unwrap_or(0);
    let ratios = pts.ratios().map(|(r_ab, r_bc, r_cd, r_ad)| {
        serde_json::json!({ "ab": r_ab, "bc": r_bc, "cd": r_cd, "ad": r_ad })
    });
    let _ = Decimal::ZERO; // keep import tidy with shared module header
    HarmonicPattern {
        slot,
        subkind,
        direction,
        start_bar,
        end_bar,
        start_time,
        end_time,
        invalidated: false,
        anchors,
        score: Some(score),
        ratios,
        extension: Some(spec.extension),
    }
}
