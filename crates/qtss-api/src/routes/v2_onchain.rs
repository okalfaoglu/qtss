//! `GET /v2/onchain` + `/v2/onchain/latest` — Faz 7.7 / C1.
//!
//! Read path for the v2 onchain pipeline. The worker
//! `v2_onchain_loop` writes per-symbol aggregate rows into
//! `qtss_v2_onchain_metrics`; these endpoints just project them out
//! for the GUI. No recomputation, no joins (CLAUDE.md #3).
//!
//! - `GET /v2/onchain?symbol=BTCUSDT&limit=200` — recent history for
//!   one symbol (chart sparkline + details list).
//! - `GET /v2/onchain/latest` — most recent row per symbol (dashboard
//!   cards). One pass via DISTINCT ON in SQL.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct OnchainQuery {
    pub symbol: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct OnchainFeed {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<OnchainEntry>,
}

#[derive(Debug, Serialize)]
pub struct OnchainEntry {
    pub id: String,
    pub symbol: String,
    pub computed_at: DateTime<Utc>,
    pub derivatives_score: Option<f64>,
    pub stablecoin_score: Option<f64>,
    pub chain_score: Option<f64>,
    pub aggregate_score: f64,
    pub direction: String,
    pub confidence: f64,
    pub details: Vec<String>,
    pub raw_meta: JsonValue,
}

pub fn v2_onchain_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/onchain", get(get_onchain_history))
        .route("/v2/onchain/latest", get(get_onchain_latest))
}

async fn get_onchain_history(
    State(st): State<SharedState>,
    Query(q): Query<OnchainQuery>,
) -> Result<Json<OnchainFeed>, ApiError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 2_000);

    // Two query shapes — symbol-filtered vs all-symbols. Kept inline
    // since it's literally one WHERE clause.
    let rows: Vec<DbRow> = if let Some(sym) = q.symbol.as_deref() {
        sqlx::query_as::<_, DbRow>(
            r#"SELECT id, symbol, computed_at,
                      derivatives_score, stablecoin_score, chain_score,
                      aggregate_score, direction, confidence, raw_meta
                 FROM qtss_v2_onchain_metrics
                WHERE symbol = $1
                ORDER BY computed_at DESC
                LIMIT $2"#,
        )
        .bind(sym.trim().to_uppercase())
        .bind(limit)
        .fetch_all(&st.pool)
        .await?
    } else {
        sqlx::query_as::<_, DbRow>(
            r#"SELECT id, symbol, computed_at,
                      derivatives_score, stablecoin_score, chain_score,
                      aggregate_score, direction, confidence, raw_meta
                 FROM qtss_v2_onchain_metrics
                ORDER BY computed_at DESC
                LIMIT $1"#,
        )
        .bind(limit)
        .fetch_all(&st.pool)
        .await?
    };

    Ok(Json(OnchainFeed {
        generated_at: Utc::now(),
        entries: rows.into_iter().map(row_to_entry).collect(),
    }))
}

/// Latest row per symbol — dashboard card view.
async fn get_onchain_latest(
    State(st): State<SharedState>,
) -> Result<Json<OnchainFeed>, ApiError> {
    let rows: Vec<DbRow> = sqlx::query_as::<_, DbRow>(
        r#"SELECT DISTINCT ON (symbol)
                  id, symbol, computed_at,
                  derivatives_score, stablecoin_score, chain_score,
                  aggregate_score, direction, confidence, raw_meta
             FROM qtss_v2_onchain_metrics
            ORDER BY symbol, computed_at DESC"#,
    )
    .fetch_all(&st.pool)
    .await?;

    Ok(Json(OnchainFeed {
        generated_at: Utc::now(),
        entries: rows.into_iter().map(row_to_entry).collect(),
    }))
}

#[derive(sqlx::FromRow)]
struct DbRow {
    id: uuid::Uuid,
    symbol: String,
    computed_at: DateTime<Utc>,
    derivatives_score: Option<f64>,
    stablecoin_score: Option<f64>,
    chain_score: Option<f64>,
    aggregate_score: f64,
    direction: String,
    confidence: f64,
    raw_meta: JsonValue,
}

fn row_to_entry(row: DbRow) -> OnchainEntry {
    let details = row
        .raw_meta
        .get("details")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    OnchainEntry {
        id: row.id.to_string(),
        symbol: row.symbol,
        computed_at: row.computed_at,
        derivatives_score: row.derivatives_score,
        stablecoin_score: row.stablecoin_score,
        chain_score: row.chain_score,
        aggregate_score: row.aggregate_score,
        direction: row.direction,
        confidence: row.confidence,
        details,
        raw_meta: row.raw_meta,
    }
}
