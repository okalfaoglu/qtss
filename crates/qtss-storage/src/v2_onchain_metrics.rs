//! Faz 7.7 / B3 — `qtss_v2_onchain_metrics` repo.
//!
//! Tiny by design: insert one row per fetcher tick + read latest row
//! per symbol with a stale-after filter. The TBM bridge calls
//! [`fetch_latest_for_tbm`] every detector pass.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct V2OnchainMetricsRow {
    pub id: Uuid,
    pub symbol: String,
    pub computed_at: DateTime<Utc>,
    pub derivatives_score: Option<f64>,
    pub stablecoin_score: Option<f64>,
    pub chain_score: Option<f64>,
    pub aggregate_score: f64,
    pub direction: String,
    pub confidence: f64,
    pub raw_meta: JsonValue,
}

#[derive(Debug, Clone, Default)]
pub struct V2OnchainMetricsInsert {
    pub symbol: String,
    pub derivatives_score: Option<f64>,
    pub stablecoin_score: Option<f64>,
    pub chain_score: Option<f64>,
    pub aggregate_score: f64,
    pub direction: String,
    pub confidence: f64,
    pub raw_meta: JsonValue,
}

pub async fn insert_v2_onchain_metrics(
    pool: &PgPool,
    row: &V2OnchainMetricsInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO qtss_v2_onchain_metrics (
            symbol, derivatives_score, stablecoin_score, chain_score,
            aggregate_score, direction, confidence, raw_meta
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
        RETURNING id
        "#,
    )
    .bind(row.symbol.trim().to_uppercase())
    .bind(row.derivatives_score)
    .bind(row.stablecoin_score)
    .bind(row.chain_score)
    .bind(row.aggregate_score)
    .bind(&row.direction)
    .bind(row.confidence)
    .bind(&row.raw_meta)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn fetch_latest_v2_onchain_metrics(
    pool: &PgPool,
    symbol: &str,
) -> Result<Option<V2OnchainMetricsRow>, StorageError> {
    let sym = symbol.trim().to_uppercase();
    let row = sqlx::query_as::<_, V2OnchainMetricsRow>(
        r#"SELECT id, symbol, computed_at,
                  derivatives_score, stablecoin_score, chain_score,
                  aggregate_score, direction, confidence, raw_meta
             FROM qtss_v2_onchain_metrics
            WHERE symbol = $1
            ORDER BY computed_at DESC
            LIMIT 1"#,
    )
    .bind(&sym)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// TBM bridge helper: returns the latest row only when it is fresher
/// than `stale_after`. Older rows act as if no data existed so the
/// pillar mutes itself instead of feeding stale signal into the score.
pub async fn fetch_latest_for_tbm(
    pool: &PgPool,
    symbol: &str,
    stale_after: Duration,
) -> Result<Option<V2OnchainMetricsRow>, StorageError> {
    let Some(row) = fetch_latest_v2_onchain_metrics(pool, symbol).await? else {
        return Ok(None);
    };
    if Utc::now() - row.computed_at > stale_after {
        return Ok(None);
    }
    Ok(Some(row))
}
