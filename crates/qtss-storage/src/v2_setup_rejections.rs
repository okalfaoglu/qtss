//! Faz 8.0 — `qtss_v2_setup_rejections` repo. Audit trail for the
//! allocator. Every time a candidate setup is refused (total risk
//! cap / max concurrent / correlation cap) we insert a row so the
//! post-mortem tooling can answer "why didn't we trade X?".

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct V2SetupRejectionRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub venue_class: String,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub profile: String,
    pub direction: String,
    pub reject_reason: String,
    pub confluence_id: Option<Uuid>,
    pub raw_meta: JsonValue,
}

#[derive(Debug, Clone)]
pub struct V2SetupRejectionInsert {
    pub venue_class: String,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub profile: String,
    pub direction: String,
    pub reject_reason: String,
    pub confluence_id: Option<Uuid>,
    pub raw_meta: JsonValue,
}

pub async fn insert_v2_setup_rejection(
    pool: &PgPool,
    row: &V2SetupRejectionInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO qtss_v2_setup_rejections (
            venue_class, exchange, symbol, timeframe, profile,
            direction, reject_reason, confluence_id, raw_meta
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        RETURNING id
        "#,
    )
    .bind(&row.venue_class)
    .bind(&row.exchange)
    .bind(&row.symbol)
    .bind(&row.timeframe)
    .bind(&row.profile)
    .bind(&row.direction)
    .bind(&row.reject_reason)
    .bind(row.confluence_id)
    .bind(&row.raw_meta)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn list_recent_setup_rejections(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<V2SetupRejectionRow>, StorageError> {
    let rows = sqlx::query_as::<_, V2SetupRejectionRow>(
        r#"SELECT id, created_at, venue_class, exchange, symbol, timeframe,
                  profile, direction, reject_reason, confluence_id, raw_meta
             FROM qtss_v2_setup_rejections
            ORDER BY created_at DESC
            LIMIT $1"#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
