//! SPEC_ONCHAIN_SIGNALS §3.2 — `onchain_signal_scores` satırları.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OnchainSignalScoreRow {
    pub id: Uuid,
    pub symbol: String,
    pub computed_at: DateTime<Utc>,
    pub funding_score: Option<f64>,
    pub oi_score: Option<f64>,
    pub ls_ratio_score: Option<f64>,
    pub taker_vol_score: Option<f64>,
    pub exchange_netflow_score: Option<f64>,
    pub exchange_balance_score: Option<f64>,
    pub hl_bias_score: Option<f64>,
    pub hl_whale_score: Option<f64>,
    pub liquidation_score: Option<f64>,
    pub nansen_sm_score: Option<f64>,
    pub nansen_netflow_score: Option<f64>,
    pub nansen_perp_score: Option<f64>,
    pub nansen_buyer_quality_score: Option<f64>,
    pub tvl_trend_score: Option<f64>,
    pub aggregate_score: f64,
    pub confidence: f64,
    pub direction: String,
    pub market_regime: Option<String>,
    pub conflict_detected: bool,
    pub conflict_detail: Option<String>,
    pub snapshot_keys: Vec<String>,
    pub meta_json: Option<JsonValue>,
}

#[derive(Debug, Clone, Default)]
pub struct OnchainSignalScoreInsert {
    pub symbol: String,
    pub funding_score: Option<f64>,
    pub oi_score: Option<f64>,
    pub ls_ratio_score: Option<f64>,
    pub taker_vol_score: Option<f64>,
    pub exchange_netflow_score: Option<f64>,
    pub exchange_balance_score: Option<f64>,
    pub hl_bias_score: Option<f64>,
    pub hl_whale_score: Option<f64>,
    pub liquidation_score: Option<f64>,
    pub nansen_sm_score: Option<f64>,
    pub nansen_netflow_score: Option<f64>,
    pub nansen_perp_score: Option<f64>,
    pub nansen_buyer_quality_score: Option<f64>,
    pub tvl_trend_score: Option<f64>,
    pub aggregate_score: f64,
    pub confidence: f64,
    pub direction: String,
    pub market_regime: Option<String>,
    pub conflict_detected: bool,
    pub conflict_detail: Option<String>,
    pub snapshot_keys: Vec<String>,
    pub meta_json: Option<JsonValue>,
}

pub async fn insert_onchain_signal_score(
    pool: &PgPool,
    row: &OnchainSignalScoreInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO onchain_signal_scores (
            symbol,
            funding_score, oi_score, ls_ratio_score, taker_vol_score,
            exchange_netflow_score, exchange_balance_score,
            hl_bias_score, hl_whale_score, liquidation_score,
            nansen_sm_score,
            nansen_netflow_score, nansen_perp_score, nansen_buyer_quality_score,
            tvl_trend_score,
            aggregate_score, confidence, direction,
            market_regime, conflict_detected, conflict_detail,
            snapshot_keys, meta_json
        ) VALUES (
            $1,
            $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
            $12, $13, $14, $15,
            $16, $17, $18, $19, $20, $21, $22, $23
        )
        RETURNING id
        "#,
    )
    .bind(&row.symbol)
    .bind(row.funding_score)
    .bind(row.oi_score)
    .bind(row.ls_ratio_score)
    .bind(row.taker_vol_score)
    .bind(row.exchange_netflow_score)
    .bind(row.exchange_balance_score)
    .bind(row.hl_bias_score)
    .bind(row.hl_whale_score)
    .bind(row.liquidation_score)
    .bind(row.nansen_sm_score)
    .bind(row.nansen_netflow_score)
    .bind(row.nansen_perp_score)
    .bind(row.nansen_buyer_quality_score)
    .bind(row.tvl_trend_score)
    .bind(row.aggregate_score)
    .bind(row.confidence)
    .bind(&row.direction)
    .bind(&row.market_regime)
    .bind(row.conflict_detected)
    .bind(&row.conflict_detail)
    .bind(&row.snapshot_keys)
    .bind(&row.meta_json)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn fetch_latest_onchain_signal_score(
    pool: &PgPool,
    symbol: &str,
) -> Result<Option<OnchainSignalScoreRow>, StorageError> {
    let sym = symbol.trim().to_uppercase();
    let row = sqlx::query_as::<_, OnchainSignalScoreRow>(
        r#"SELECT id, symbol, computed_at,
                  funding_score, oi_score, ls_ratio_score, taker_vol_score,
                  exchange_netflow_score, exchange_balance_score,
                  hl_bias_score, hl_whale_score, liquidation_score,
                  nansen_sm_score,
                  nansen_netflow_score, nansen_perp_score, nansen_buyer_quality_score,
                  tvl_trend_score,
                  aggregate_score, confidence, direction,
                  market_regime, conflict_detected, conflict_detail,
                  snapshot_keys, meta_json
           FROM onchain_signal_scores
           WHERE symbol = $1
           ORDER BY computed_at DESC
           LIMIT 1"#,
    )
    .bind(&sym)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_onchain_signal_scores_history(
    pool: &PgPool,
    symbol: &str,
    limit: i64,
) -> Result<Vec<OnchainSignalScoreRow>, StorageError> {
    let sym = symbol.trim().to_uppercase();
    let lim = limit.clamp(1, 500);
    let rows = sqlx::query_as::<_, OnchainSignalScoreRow>(
        r#"SELECT id, symbol, computed_at,
                  funding_score, oi_score, ls_ratio_score, taker_vol_score,
                  exchange_netflow_score, exchange_balance_score,
                  hl_bias_score, hl_whale_score, liquidation_score,
                  nansen_sm_score,
                  nansen_netflow_score, nansen_perp_score, nansen_buyer_quality_score,
                  tvl_trend_score,
                  aggregate_score, confidence, direction,
                  market_regime, conflict_detected, conflict_detail,
                  snapshot_keys, meta_json
           FROM onchain_signal_scores
           WHERE symbol = $1
           ORDER BY computed_at DESC
           LIMIT $2"#,
    )
    .bind(&sym)
    .bind(lim)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// SPEC §10 — `days` günden eski skor satırlarını siler.
pub async fn delete_onchain_signal_scores_older_than(
    pool: &PgPool,
    days: i32,
) -> Result<u64, StorageError> {
    let d = days.max(1);
    let res = sqlx::query(
        r#"DELETE FROM onchain_signal_scores
           WHERE computed_at < NOW() - make_interval(days => $1)"#,
    )
    .bind(d)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}
