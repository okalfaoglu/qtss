//! `GET /v2/elliott-db/{venue}/{symbol}/{tf}` — Elliott patterns
//! rendered from the **persisted `detections` table** rather than
//! live-computed via `luxalgo_pine_port::run`. Same response shape
//! as `/v2/elliott` so the chart can swap sources with just a URL
//! change; fib_band / break_markers / pivots arrays stay empty
//! because those are chart-only ephemerals the writer doesn't persist.
//!
//! Purpose: verify round-trip correctness — what the writer stored
//! is what a reader sees, rendered identically to the live path.
//! Once trusted, downstream consumers (setup engine, backtests,
//! offline analysis) can read from here without re-running the
//! detector.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use qtss_elliott::luxalgo_pine_port::{
    AbcPattern, LevelOutput, MotivePattern, PinePortOutput, PivotPoint, TrianglePattern,
};
use qtss_storage::market_bars;

use crate::error::ApiError;
use crate::state::SharedState;

use super::v2_elliott::{ElliottCandle, ElliottResponse};

#[derive(Debug, Deserialize)]
pub struct ElliottDbQuery {
    pub limit: Option<i64>,
    pub segment: Option<String>,
    /// Optional — restrict to one slot (0..=4).
    pub slot: Option<i16>,
    /// Optional — only include patterns whose direction matches (±1).
    pub direction: Option<i16>,
}

pub fn v2_elliott_db_router() -> Router<SharedState> {
    Router::new().route(
        "/v2/elliott-db/{venue}/{symbol}/{tf}",
        get(get_elliott_db),
    )
}

/// Raw detection row flattened from the `detections` table. Subset of
/// columns needed to reconstruct PinePortOutput.
#[derive(Debug)]
struct DetectionRow {
    slot: i16,
    pattern_family: String,
    subkind: String,
    direction: i16,
    start_bar: i64,
    end_bar: i64,
    anchors: serde_json::Value,
    live: Option<bool>,
    next_hint: Option<bool>,
    invalidated: bool,
}

async fn load_detections(
    pool: &sqlx::PgPool,
    venue: &str,
    segment: &str,
    symbol: &str,
    tf: &str,
    slot_filter: Option<i16>,
    direction_filter: Option<i16>,
) -> Result<Vec<DetectionRow>, ApiError> {
    let rows = sqlx::query(
        r#"SELECT slot, pattern_family, subkind, direction,
                  start_bar, end_bar, anchors, live, next_hint, invalidated
             FROM detections
            WHERE exchange = $1 AND segment = $2 AND symbol = $3
              AND timeframe = $4 AND mode = 'live'
              AND ($5::smallint IS NULL OR slot = $5)
              AND ($6::smallint IS NULL OR direction = $6)
            ORDER BY slot, start_bar"#,
    )
    .bind(venue)
    .bind(segment)
    .bind(symbol)
    .bind(tf)
    .bind(slot_filter)
    .bind(direction_filter)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| DetectionRow {
            slot: r.get("slot"),
            pattern_family: r.get("pattern_family"),
            subkind: r.get("subkind"),
            direction: r.get("direction"),
            start_bar: r.get("start_bar"),
            end_bar: r.get("end_bar"),
            anchors: r.try_get("anchors").unwrap_or(serde_json::Value::Null),
            live: r.try_get("live").ok(),
            next_hint: r.try_get("next_hint").ok(),
            invalidated: r.get("invalidated"),
        })
        .collect())
}

/// Deserialize the anchors JSON (array of PivotPoint) back into a
/// concrete Vec. On malformed JSON returns empty — the pattern will
/// be skipped downstream rather than panicking the whole response.
fn parse_anchors(value: &serde_json::Value) -> Vec<PivotPoint> {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

/// Extract per-anchor `open_time` from the JSON if the writer included
/// it (post-migration 0215 format). Index-aligned with `parse_anchors`
/// output. Empty vec when the field is missing (legacy rows). Callers
/// should fall back to the stale `bar_index` in that case.
fn parse_anchor_times(value: &serde_json::Value) -> Vec<Option<DateTime<Utc>>> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|item| {
                    item.get("time")
                        .and_then(|t| serde_json::from_value(t.clone()).ok())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Remap each anchor's `bar_index` to match the current chart window by
/// matching its stored `time` to a candle's `open_time`. Returns `None`
/// when any anchor falls outside the current window — the pattern
/// can't be fully drawn so the caller drops it.
fn remap_anchors_to_window(
    mut anchors: Vec<PivotPoint>,
    times: &[Option<DateTime<Utc>>],
    candles: &[ElliottCandle],
) -> Option<Vec<PivotPoint>> {
    if anchors.len() != times.len() {
        // Legacy row without per-anchor times — fall back to stored
        // bar_index. This will look misaligned but preserves backward
        // compat with rows written before migration 0215.
        return Some(anchors);
    }
    for (i, t_opt) in times.iter().enumerate() {
        let Some(t) = t_opt else {
            return None;
        };
        // Candles are chronologically ordered; binary_search_by is O(log n).
        let pos = candles.binary_search_by(|c| c.time.cmp(t)).ok();
        let Some(idx) = pos else {
            return None;
        };
        anchors[i].bar_index = idx as i64;
    }
    Some(anchors)
}

/// Group detections by (slot, family) and reconstruct PinePortOutput.
/// Each row's anchors are remapped into the current chart bar-window
/// via per-anchor `time` → `bar_index` lookup. Patterns whose anchors
/// fall outside the window get dropped rather than rendered with
/// stale writer-relative indices.
///
/// ABCs attach to the motive sharing the same slot whose `end_bar`
/// equals the ABC's `start_bar` (the writer's canonical linkage: the
/// ABC's first anchor coincides with its parent motive's p5).
fn build_pine_output(
    rows: Vec<DetectionRow>,
    candles: &[ElliottCandle],
) -> PinePortOutput {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct SlotBuckets {
        motives: Vec<(DetectionRow, Vec<PivotPoint>)>,
        abcs: Vec<(DetectionRow, Vec<PivotPoint>)>,
        triangles: Vec<(DetectionRow, Vec<PivotPoint>)>,
    }
    let mut by_slot: BTreeMap<i16, SlotBuckets> = BTreeMap::new();

    for row in rows {
        let anchors = parse_anchors(&row.anchors);
        let times = parse_anchor_times(&row.anchors);
        // Remap to current window; drop the pattern entirely if any
        // anchor is outside the visible candles.
        let Some(mut remapped) = remap_anchors_to_window(anchors, &times, candles) else {
            continue;
        };
        // After remap, also refresh start_bar / end_bar on the row
        // to match the new window so ABC-to-motive linkage below
        // stays consistent.
        if let (Some(first), Some(last)) = (remapped.first(), remapped.last()) {
            let mut r = row;
            r.start_bar = first.bar_index;
            r.end_bar = last.bar_index;
            // Shift subsequent borrows of remapped — take ownership via swap.
            let taken = std::mem::take(&mut remapped);
            let bucket = by_slot.entry(r.slot).or_default();
            match r.pattern_family.as_str() {
                "motive" => bucket.motives.push((r, taken)),
                "abc" => bucket.abcs.push((r, taken)),
                "triangle" => bucket.triangles.push((r, taken)),
                _ => {}
            }
        }
    }

    // Each slot gets its own LevelOutput. Palette mirrors v2_elliott
    // defaults so the chart colours stay consistent across sources.
    let palette: [&str; 5] = ["#ef4444", "#3b82f6", "#e5e7eb", "#f59e0b", "#a78bfa"];
    let lengths: [usize; 5] = [3, 5, 8, 13, 21];

    let mut levels: Vec<LevelOutput> = Vec::new();
    for (slot, bucket) in by_slot {
        let idx = slot.max(0) as usize;
        let color = palette.get(idx).copied().unwrap_or("#888888").to_string();
        let length = lengths.get(idx).copied().unwrap_or(3);

        let mut motives: Vec<MotivePattern> = Vec::with_capacity(bucket.motives.len());
        for (m_row, m_anchors) in bucket.motives {
            if m_anchors.len() != 6 {
                continue;
            }
            // Match ABC: same slot (already), whose start_bar falls
            // inside or immediately after this motive's bar range.
            // Pine port writes the ABC with start_bar = motive.p5
            // (parent's last anchor), so equality on start_bar is the
            // canonical match.
            let matched_abc = bucket
                .abcs
                .iter()
                .find(|(a, _)| a.start_bar == m_row.end_bar)
                .map(|(a, anchors)| AbcPattern {
                    direction: a.direction as i8,
                    anchors: anchors_to_array_4(anchors),
                    invalidated: a.invalidated,
                    subkind: Some(a.subkind.clone()),
                });
            motives.push(MotivePattern {
                direction: m_row.direction as i8,
                anchors: anchors_to_array_6(&m_anchors),
                live: m_row.live.unwrap_or(false),
                next_hint: m_row.next_hint.unwrap_or(false),
                abc: matched_abc,
                break_box: None,   // not persisted
                next_marker: None, // not persisted
            });
        }

        let mut triangles: Vec<TrianglePattern> = Vec::with_capacity(bucket.triangles.len());
        for (t_row, t_anchors) in bucket.triangles {
            if t_anchors.len() != 6 {
                continue;
            }
            triangles.push(TrianglePattern {
                direction: t_row.direction as i8,
                subkind: t_row.subkind.clone(),
                anchors: anchors_to_array_6(&t_anchors),
                invalidated: t_row.invalidated,
            });
        }

        levels.push(LevelOutput {
            length,
            color,
            pivots: Vec::new(),         // not persisted; chart falls back to /v2/zigzag for dots
            motives,
            break_markers: Vec::new(),  // not persisted
            fib_band: None,             // computed live only
            triangles,
        });
    }

    PinePortOutput {
        bar_count: candles.len() as i64,
        levels,
    }
}

/// The Pine port's `MotivePattern` wants `[PivotPoint; 6]`. Anchors
/// loaded from JSON come as `Vec<PivotPoint>`; convert with length
/// guard. Caller must verify `.len() == 6` before invoking.
fn anchors_to_array_6(v: &[PivotPoint]) -> [PivotPoint; 6] {
    [
        v[0].clone(), v[1].clone(), v[2].clone(),
        v[3].clone(), v[4].clone(), v[5].clone(),
    ]
}
fn anchors_to_array_4(v: &[PivotPoint]) -> [PivotPoint; 4] {
    [v[0].clone(), v[1].clone(), v[2].clone(), v[3].clone()]
}

async fn get_elliott_db(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<ElliottDbQuery>,
) -> Result<Json<ElliottResponse>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(1000).clamp(1, 5_000);

    // Candles — same loader as /v2/elliott so bar_index alignment is
    // guaranteed regardless of which path the chart is on.
    let rows =
        market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, limit).await?;
    let candles: Vec<ElliottCandle> = rows
        .into_iter()
        .rev()
        .enumerate()
        .map(|(i, r)| ElliottCandle {
            time: r.open_time,
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume,
            bar_index: i as i64,
        })
        .collect();
    let bar_count = candles.len() as i64;

    let detections = load_detections(
        &st.pool, &venue, &segment, &symbol, &tf, q.slot, q.direction,
    )
    .await?;
    let _ = bar_count; // consumed via candles.len() inside build_pine_output
    let pine = build_pine_output(detections, &candles);

    Ok(Json(ElliottResponse {
        venue,
        symbol,
        timeframe: tf,
        candles,
        pine,
    }))
}
