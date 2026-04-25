//! `GET /v2/wave-bars/{exchange}/{symbol}/{tf}` — pivot-based "wave bars".
//!
//! FAZ 25.1 — noise-cleaned candle structure for Elliott wave counting.
//!
//! Each "candle" in the returned series represents one ZigZag wave (pivot
//! → next pivot) instead of a fixed-time bar. Open = start pivot price,
//! Close = end pivot price; High and Low are derived from the underlying
//! OHLC bars between the two pivots so the wick still shows the maximum
//! excursion within the wave. Duration is preserved as an attribute (we
//! do NOT lose time information — Elliott alternation needs it) but the
//! visual X-axis is wave-index, not real time, so the user reads waves
//! at uniform spacing.
//!
//! Direction: `+1` for an upward wave (low→high pivot), `-1` for a down
//! wave (high→low pivot). The frontend colours the body accordingly.
//!
//! This endpoint complements `/v2/zigzag` — the zigzag route returns
//! pivots; this route returns the wave segments BETWEEN those pivots
//! enriched with size/duration so the frontend can render OHLC-shaped
//! candles directly.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use qtss_pivots::zigzag::{compute_pivots, Sample};
use qtss_storage::market_bars;

use crate::error::ApiError;
use crate::state::SharedState;

const DEFAULT_LENGTHS: [u32; 5] = [3, 5, 8, 13, 21];

#[derive(Debug, Deserialize)]
pub struct WaveBarsQuery {
    pub segment: Option<String>,
    /// Window of underlying OHLC bars to consider (default 1000).
    pub limit: Option<i64>,
    /// Slot 0..=4 picks Z1..Z5 (default 2 = Z3 length 8).
    pub slot: Option<u8>,
    /// Override slot length (e.g. `?length=8`); falls back to default.
    pub length: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct WaveBar {
    /// Sequential index 0, 1, 2, … — used as the synthetic X-axis on
    /// the frontend so all bars get equal visual width.
    pub index: i64,
    pub slot: u8,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub start_bar_index: i64,
    pub end_bar_index: i64,
    /// Open = price at the starting pivot.
    pub open: f64,
    /// Close = price at the ending pivot. Direction sign is
    /// `signum(close - open)`.
    pub close: f64,
    /// Maximum price reached between the two pivots (i.e. the high of
    /// the underlying bars that span the wave).
    pub high: f64,
    pub low: f64,
    /// `+1` upward wave, `-1` downward wave. Always non-zero for a
    /// confirmed pivot pair.
    pub direction: i8,
    pub duration_seconds: i64,
    /// Number of underlying OHLC bars the wave covers.
    pub bar_count: i64,
    /// Wave magnitude expressed as fraction of the median wave size in
    /// the returned set — handy for spotting outsized impulses (W3
    /// extension etc.) without computing ATR client-side.
    pub size_norm: f64,
    /// Rough volume aggregate over the wave's underlying bars, in
    /// quote-currency units (sum of close × volume). Useful for
    /// distinguishing thrust waves from drifting ones.
    pub volume_total: f64,
}

#[derive(Debug, Serialize)]
pub struct WaveBarsResponse {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub timeframe: String,
    pub slot: u8,
    pub length: u32,
    /// Sequence of completed wave bars (oldest first).
    pub waves: Vec<WaveBar>,
}

pub fn v2_wave_bars_router() -> Router<SharedState> {
    Router::new().route(
        "/v2/wave-bars/{exchange}/{symbol}/{tf}",
        get(get_wave_bars),
    )
}

async fn get_wave_bars(
    State(state): State<SharedState>,
    Path((exchange, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<WaveBarsQuery>,
) -> Result<Json<WaveBarsResponse>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(1000).clamp(100, 10_000);
    let slot = q.slot.unwrap_or(2).min(4);
    let length = q.length.unwrap_or(DEFAULT_LENGTHS[slot as usize]);

    let pool = &state.pool;

    // Pull recent OHLC bars in chronological order.
    let raw = market_bars::list_recent_bars(pool, &exchange, &segment, &symbol, &tf, limit)
        .await
        .map_err(|e| {
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("market_bars fetch failed: {e}"),
            )
        })?;
    if raw.len() < 4 {
        return Ok(Json(WaveBarsResponse {
            exchange,
            segment,
            symbol,
            timeframe: tf,
            slot,
            length,
            waves: Vec::new(),
        }));
    }
    let chrono: Vec<_> = raw.into_iter().rev().collect();

    // Build the pivot tape via the same engine the worker uses.
    let samples: Vec<Sample> = chrono
        .iter()
        .enumerate()
        .map(|(i, b)| Sample {
            bar_index: i as u64,
            time: b.open_time,
            high: b.high,
            low: b.low,
            volume: b.volume,
        })
        .collect();
    let pivots = compute_pivots(&samples, length);
    if pivots.len() < 2 {
        return Ok(Json(WaveBarsResponse {
            exchange,
            segment,
            symbol,
            timeframe: tf,
            slot,
            length,
            waves: Vec::new(),
        }));
    }

    // Convert each consecutive pivot pair into a wave bar. We iterate
    // in chronological order, so direction is determined by the END
    // pivot's kind.
    let mut waves: Vec<WaveBar> = Vec::with_capacity(pivots.len().saturating_sub(1));
    for (idx, pair) in pivots.windows(2).enumerate() {
        let p0 = &pair[0];
        let p1 = &pair[1];
        let s_idx = p0.bar_index as usize;
        let e_idx = p1.bar_index as usize;
        if e_idx <= s_idx || e_idx >= chrono.len() {
            continue;
        }
        let bar_count = (e_idx - s_idx) as i64;
        let start_time = chrono[s_idx].open_time;
        let end_time = chrono[e_idx].open_time;
        let duration_seconds = (end_time - start_time).num_seconds().max(0);
        let open = p0.price.to_f64().unwrap_or(0.0);
        let close = p1.price.to_f64().unwrap_or(0.0);
        let direction: i8 = if close >= open { 1 } else { -1 };

        // Walk the underlying OHLC slice for high / low / volume.
        let mut hi = open.max(close);
        let mut lo = open.min(close);
        let mut volume_total = 0.0f64;
        for b in &chrono[s_idx..=e_idx] {
            let h = b.high.to_f64().unwrap_or(0.0);
            let l = b.low.to_f64().unwrap_or(0.0);
            if h > hi {
                hi = h;
            }
            if l < lo {
                lo = l;
            }
            let c = b.close.to_f64().unwrap_or(0.0);
            let v = b.volume.to_f64().unwrap_or(0.0);
            volume_total += c * v;
        }

        waves.push(WaveBar {
            index: idx as i64,
            slot,
            start_time,
            end_time,
            start_bar_index: p0.bar_index as i64,
            end_bar_index: p1.bar_index as i64,
            open,
            close,
            high: hi,
            low: lo,
            direction,
            duration_seconds,
            bar_count,
            size_norm: (close - open).abs(), // patched to relative below
            volume_total,
        });
    }

    // Normalise size as fraction of median absolute wave displacement
    // — a single-pass dimensionless metric that highlights extension
    // waves (>= 1.5) and dampens minor pullbacks (<= 0.5).
    if !waves.is_empty() {
        let mut sizes: Vec<f64> = waves.iter().map(|w| (w.close - w.open).abs()).collect();
        sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = sizes[sizes.len() / 2].max(1e-9);
        for w in &mut waves {
            w.size_norm = (w.close - w.open).abs() / median;
        }
    }

    Ok(Json(WaveBarsResponse {
        exchange,
        segment,
        symbol,
        timeframe: tf,
        slot,
        length,
        waves,
    }))
}
