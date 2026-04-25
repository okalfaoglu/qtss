//! `GET /v2/zigzag/{venue}/{symbol}/{tf}` — canonical zigzag endpoint.
//!
//! Single source of truth for the GUI's LuxAlgo chart and anything
//! else that wants "what pivots are on this series at each slot".
//! Calls the same `qtss_pivots::zigzag::compute_pivots` function the
//! worker uses — so the GUI can never drift from the detection
//! pipeline's pivot view.
//!
//! Fallback strategy: for now we compute live from `market_bars` on
//! every request. A future upgrade will read from the `pivots` table
//! (written by the worker loop in Faz 6) and fall back to live-compute
//! only when the table is empty for the series — same shape either way.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use qtss_pivots::zigzag::{compute_pivots, filter_prominence, Sample};
use qtss_storage::{market_bars, market_bars_open};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ZigzagQuery {
    pub limit: Option<i64>,
    pub segment: Option<String>,
    /// Restrict to a single slot (0..=4). Omit for all five.
    pub slot: Option<u8>,
    /// Comma-separated custom lengths (overrides `system_config`).
    /// Lets the GUI tune Z1..Z5 live without a round-trip through the
    /// config write path. Up to five values; extras are ignored.
    pub lengths: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ZigzagCandle {
    pub time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub bar_index: i64,
}

#[derive(Debug, Serialize)]
pub struct ZigzagPivot {
    pub bar_index: i64,
    pub time: DateTime<Utc>,
    pub direction: i8,
    pub price: f64,
    pub volume: f64,
    /// HH/HL/LL/LH relative to the previous same-direction pivot.
    /// `None` for the first pivot of its kind.
    pub swing_tag: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct ZigzagLevel {
    pub slot: u8,
    pub length: u32,
    pub color: String,
    pub pivots: Vec<ZigzagPivot>,
    /// "Pending" pivot — the most extreme bar after the last confirmed
    /// pivot, in the opposite direction. Pine/TV draws a dashed leg
    /// from the last confirmed pivot to this one so the zigzag reaches
    /// the current bar. `None` if no bars follow the last confirmed
    /// pivot, or no pivots have confirmed yet.
    pub provisional_pivot: Option<ZigzagPivot>,
}

#[derive(Debug, Serialize)]
pub struct ZigzagResponse {
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    pub candles: Vec<ZigzagCandle>,
    pub levels: Vec<ZigzagLevel>,
}

pub fn v2_zigzag_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/zigzag/{venue}/{symbol}/{tf}", get(get_zigzag))
        // Single source of truth for the Z1..Z5 slot ladder. Frontend
        // hits this once on chart mount so the toolbar's lengths and
        // colors mirror the same `system_config.zigzag.slot_N` rows
        // the engine writers consume — no more hardcoded TS defaults
        // drifting from operator tweaks.
        .route("/v2/zigzag/slots", get(get_zigzag_slots))
}

#[derive(Debug, Serialize)]
pub struct SlotConfigDto {
    pub slot: u8,
    pub length: u32,
    pub color: String,
}

#[derive(Debug, Serialize)]
pub struct SlotConfigsResponse {
    pub slots: Vec<SlotConfigDto>,
}

async fn get_zigzag_slots(
    State(st): State<SharedState>,
) -> Result<Json<SlotConfigsResponse>, ApiError> {
    let cfgs = load_slot_configs(&st.pool).await;
    Ok(Json(SlotConfigsResponse {
        slots: cfgs
            .into_iter()
            .map(|c| SlotConfigDto {
                slot: c.slot,
                length: c.length,
                color: c.color,
            })
            .collect(),
    }))
}

#[derive(Debug, Clone)]
struct SlotConfig {
    slot: u8,
    length: u32,
    color: String,
}

/// Resolve the five configured slots from `system_config.zigzag.slot_0..slot_4`.
/// Falls back to the Fibonacci defaults if any row is missing.
async fn load_slot_configs(pool: &sqlx::PgPool) -> Vec<SlotConfig> {
    const DEFAULTS: [(u32, &str); 5] = [
        (3,  "#ef4444"),
        (5,  "#3b82f6"),
        (8,  "#e5e7eb"),
        (13, "#f59e0b"),
        (21, "#a78bfa"),
    ];

    let mut out: Vec<SlotConfig> = DEFAULTS
        .iter()
        .enumerate()
        .map(|(i, (len, col))| SlotConfig {
            slot: i as u8,
            length: *len,
            color: col.to_string(),
        })
        .collect();

    for i in 0..5u8 {
        let key = format!("slot_{i}");
        let row = sqlx::query(
            "SELECT value FROM system_config WHERE module = 'zigzag' AND config_key = $1",
        )
        .bind(&key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        if let Some(row) = row {
            let val: serde_json::Value = row.try_get("value").unwrap_or(serde_json::Value::Null);
            if let Some(len) = val.get("length").and_then(|v| v.as_u64()) {
                out[i as usize].length = len.max(1) as u32;
            }
            if let Some(col) = val.get("color").and_then(|v| v.as_str()) {
                out[i as usize].color = col.to_string();
            }
        }
    }
    out
}

/// Pull `system_config.zigzag.min_prominence_pct` (swing-filter
/// threshold). Any pivot pair whose `|Δprice| / prev_price` is below
/// this percentage gets absorbed into the surrounding swing. Default
/// 0.0 (no filter) if the row is missing or malformed.
async fn load_min_prominence_pct(pool: &sqlx::PgPool) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'zigzag' AND config_key = 'min_prominence_pct'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.0; };
    let val: serde_json::Value = row.try_get("value").unwrap_or(serde_json::Value::Null);
    val.get("pct")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .max(0.0)
}

fn classify_swing(
    prev: Option<&ZigzagPivot>,
    kind_dir: i8,
    price: f64,
) -> Option<&'static str> {
    let prev = prev?;
    if prev.direction.signum() != kind_dir {
        return None;
    }
    if kind_dir == 1 {
        Some(if price >= prev.price { "HH" } else { "LH" })
    } else {
        Some(if price <= prev.price { "LL" } else { "HL" })
    }
}

async fn get_zigzag(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<ZigzagQuery>,
) -> Result<Json<ZigzagResponse>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(1000).clamp(1, 5_000);

    let rows = market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, limit)
        .await?;

    // DB returns newest-first — flip to chronological.
    let mut candles: Vec<ZigzagCandle> = rows
        .into_iter()
        .rev()
        .enumerate()
        .map(|(i, r)| ZigzagCandle {
            time: r.open_time,
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume,
            bar_index: i as i64,
        })
        .collect();

    // Append the live (still-forming) bar from market_bars_open so the
    // chart and zigzag reach the current tick. Worker overwrites that
    // row on every WebSocket frame; API merges it only when it's newer
    // than the last archived bar.
    if let Ok(Some(open_bar)) =
        market_bars_open::get_open_bar(&st.pool, &venue, &segment, &symbol, &tf).await
    {
        let is_newer = candles
            .last()
            .map(|c| open_bar.open_time > c.time)
            .unwrap_or(true);
        if is_newer {
            let next_idx = candles.len() as i64;
            candles.push(ZigzagCandle {
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

    let mut slots = load_slot_configs(&st.pool).await;
    // Apply `?lengths=` override (up to 5 values, slot by slot).
    if let Some(csv) = q.lengths.as_deref() {
        for (i, part) in csv.split(',').take(slots.len()).enumerate() {
            if let Ok(n) = part.trim().parse::<u32>() {
                if n >= 1 {
                    slots[i].length = n;
                }
            }
        }
    }
    let filtered: Vec<&SlotConfig> = match q.slot {
        Some(s) if (s as usize) < slots.len() => vec![&slots[s as usize]],
        _ => slots.iter().collect(),
    };

    let min_prominence_pct = load_min_prominence_pct(&st.pool).await;

    let mut levels: Vec<ZigzagLevel> = Vec::with_capacity(filtered.len());
    for cfg in filtered {
        let raw = compute_pivots(&samples, cfg.length);
        let confirmed = filter_prominence(&raw, min_prominence_pct);
        let mut out_pivots: Vec<ZigzagPivot> = Vec::with_capacity(confirmed.len());
        for cp in confirmed {
            // Pine's `newDir` — ±1 normal, ±2 strong (HH/LL).
            let direction: i8 = cp.direction;
            let sign: i8 = direction.signum();
            let price = cp.price.to_f64().unwrap_or(0.0);
            let prev_same = out_pivots.iter().rev().find(|p| p.direction.signum() == sign);
            let swing_tag = classify_swing(prev_same, sign, price);
            out_pivots.push(ZigzagPivot {
                bar_index: cp.bar_index as i64,
                time: cp.time,
                direction,
                price,
                volume: cp.volume_at_pivot.to_f64().unwrap_or(0.0),
                swing_tag,
            });
        }
        // Provisional pivot = most extreme opposite-direction bar since
        // the last confirmed pivot. Lets the polyline reach the current
        // bar (what TV shows as a dashed pending leg).
        let provisional_pivot = out_pivots.last().and_then(|last| {
            let start = last.bar_index as usize + 1;
            if start >= candles.len() {
                return None;
            }
            let look_for_low = last.direction == 1;
            let mut best_idx = start;
            let mut best_price = if look_for_low {
                candles[start].low.to_f64().unwrap_or(0.0)
            } else {
                candles[start].high.to_f64().unwrap_or(0.0)
            };
            for i in (start + 1)..candles.len() {
                let p = if look_for_low {
                    candles[i].low.to_f64().unwrap_or(0.0)
                } else {
                    candles[i].high.to_f64().unwrap_or(0.0)
                };
                let better = if look_for_low { p < best_price } else { p > best_price };
                if better {
                    best_idx = i;
                    best_price = p;
                }
            }
            let c = &candles[best_idx];
            Some(ZigzagPivot {
                bar_index: c.bar_index,
                time: c.time,
                direction: if look_for_low { -1 } else { 1 },
                price: best_price,
                volume: c.volume.to_f64().unwrap_or(0.0),
                swing_tag: None,
            })
        });

        levels.push(ZigzagLevel {
            slot: cfg.slot,
            length: cfg.length,
            color: cfg.color.clone(),
            pivots: out_pivots,
            provisional_pivot,
        });
    }

    Ok(Json(ZigzagResponse {
        venue,
        symbol,
        timeframe: tf,
        candles,
        levels,
    }))
}
