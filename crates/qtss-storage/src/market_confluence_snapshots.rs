//! Append-only history of derived confluence scores per `engine_symbols` row (PLAN Phase B).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MarketConfluenceSnapshotRow {
    pub id: Uuid,
    pub engine_symbol_id: Uuid,
    pub computed_at: DateTime<Utc>,
    pub schema_version: i32,
    pub regime: Option<String>,
    pub composite_score: f64,
    pub confidence_0_100: i32,
    pub scores_json: JsonValue,
    pub conflicts_json: JsonValue,
    /// Full confluence payload when column present (migration `0029`).
    pub confluence_payload_json: Option<JsonValue>,
}

#[derive(Debug, Clone)]
pub struct MarketConfluenceSnapshotInsert {
    pub engine_symbol_id: Uuid,
    pub schema_version: i32,
    pub regime: Option<String>,
    pub composite_score: f64,
    pub confidence_0_100: i32,
    pub scores_json: JsonValue,
    pub conflicts_json: JsonValue,
    pub confluence_payload_json: Option<JsonValue>,
}

pub async fn insert_market_confluence_snapshot(
    pool: &PgPool,
    row: &MarketConfluenceSnapshotInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO market_confluence_snapshots (
               engine_symbol_id, schema_version, regime, composite_score, confidence_0_100,
               scores_json, conflicts_json, confluence_payload_json
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           RETURNING id"#,
    )
    .bind(row.engine_symbol_id)
    .bind(row.schema_version)
    .bind(&row.regime)
    .bind(row.composite_score)
    .bind(row.confidence_0_100)
    .bind(&row.scores_json)
    .bind(&row.conflicts_json)
    .bind(&row.confluence_payload_json)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Newest first.
pub async fn list_market_confluence_snapshots_for_symbol(
    pool: &PgPool,
    engine_symbol_id: Uuid,
    limit: i64,
) -> Result<Vec<MarketConfluenceSnapshotRow>, StorageError> {
    let lim = limit.clamp(1, 500);
    let rows = sqlx::query_as::<_, MarketConfluenceSnapshotRow>(
        r#"SELECT id, engine_symbol_id, computed_at, schema_version, regime, composite_score,
                  confidence_0_100, scores_json, conflicts_json, confluence_payload_json
           FROM market_confluence_snapshots
           WHERE engine_symbol_id = $1
           ORDER BY computed_at DESC
           LIMIT $2"#,
    )
    .bind(engine_symbol_id)
    .bind(lim)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
