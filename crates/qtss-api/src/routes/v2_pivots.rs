//! `GET /v2/pivots/{venue}/{symbol}/{tf}` — multi-level ZigZag pivots for chart overlay.
//!
//! Returns all four cascaded pivot levels (L0 micro → L3 macro) the
//! `qtss-pivots` engine produces. The chart panel renders each level as
//! a separate line series so the operator can toggle L0/L1/L2/L3
//! independently (CLAUDE.md #2 — per-level style is driven by config,
//! not hardcoded).
//!
//! Data source: `pivots` table — written by `pivot_writer_loop`. Joined
//! through `engine_symbols` so the (exchange, symbol, interval) URL
//! params translate to the series UUID the table keys on.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::state::SharedState;

const LEVELS: [&str; 4] = ["L0", "L1", "L2", "L3"];

#[derive(Debug, Deserialize)]
pub struct PivotsQuery {
    pub segment: Option<String>,
    pub limit: Option<i64>,
    /// Optional comma-separated level filter, e.g. `?levels=L1,L2`.
    /// Empty/missing → return all four levels.
    pub levels: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PivotPoint {
    pub bar_index: i64,
    pub open_time: DateTime<Utc>,
    pub price: Decimal,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swing_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PivotLevelSeries {
    pub level: String,
    pub points: Vec<PivotPoint>,
}

#[derive(Debug, Serialize)]
pub struct PivotsResponse {
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    pub levels: Vec<PivotLevelSeries>,
}

pub fn v2_pivots_router() -> Router<SharedState> {
    Router::new().route("/v2/pivots/{venue}/{symbol}/{tf}", get(get_pivots))
}

async fn get_pivots(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<PivotsQuery>,
) -> Result<Json<PivotsResponse>, ApiError> {
    let _segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(2_000).clamp(1, 20_000);

    // Parse level filter (lookup-style — no if/else chain per CLAUDE.md #1)
    let requested_levels: Vec<&str> = match q.levels.as_deref() {
        None | Some("") => LEVELS.to_vec(),
        Some(csv) => csv
            .split(',')
            .map(str::trim)
            .filter(|s| LEVELS.contains(s))
            .collect(),
    };

    // ORDER BY bar_index DESC + LIMIT + outer ASC gives us the newest N
    // rows in chronological order — the only shape the chart overlay
    // needs. Reading `pivots` JOIN `engine_symbols` so the old
    // (exchange, symbol, timeframe) key keeps working without a segment
    // param on the URL; segment is fixed to `futures` for now (matches
    // every live series) but explicit enough to fix later.
    let mut out: Vec<PivotLevelSeries> = Vec::with_capacity(requested_levels.len());
    for level in requested_levels {
        let level_i: i16 = level[1..].parse().unwrap_or(0);
        let rows = sqlx::query_as::<
            _,
            (
                i64,
                chrono::DateTime<chrono::Utc>,
                rust_decimal::Decimal,
                i16,
                Option<String>,
            ),
        >(
            r#"
            SELECT bar_index, open_time, price, direction, swing_tag
              FROM (
                SELECT p.bar_index, p.open_time, p.price, p.direction, p.swing_tag
                  FROM pivots p
                  JOIN engine_symbols es ON es.id = p.engine_symbol_id
                 WHERE es.exchange   = $1
                   AND es.symbol     = $2
                   AND es."interval" = $3
                   AND p.level       = $4
                 ORDER BY p.bar_index DESC
                 LIMIT $5
              ) t
             ORDER BY bar_index ASC
            "#,
        )
        .bind(&venue)
        .bind(&symbol)
        .bind(&tf)
        .bind(level_i)
        .bind(limit)
        .fetch_all(&st.pool)
        .await
        .map_err(|e| {
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("pivots read failed: {e}"),
            )
        })?;
        let points = rows
            .into_iter()
            .map(|(bar_index, open_time, price, direction, swing_tag)| PivotPoint {
                bar_index,
                open_time,
                price,
                kind: if direction >= 1 { "High".to_string() } else { "Low".to_string() },
                swing_type: swing_tag,
            })
            .collect();
        out.push(PivotLevelSeries {
            level: level.to_string(),
            points,
        });
    }

    Ok(Json(PivotsResponse {
        venue,
        symbol,
        timeframe: tf,
        levels: out,
    }))
}
