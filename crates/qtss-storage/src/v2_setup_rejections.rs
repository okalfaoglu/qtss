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
    list_setup_rejections_filtered(pool, &RejectionFilter { limit, ..Default::default() }).await
}

/// Filter set for the Faz 9.1.3 Confluence Inspector.
#[derive(Debug, Clone, Default)]
pub struct RejectionFilter {
    pub limit: i64,
    pub venue_class: Option<String>,
    pub reason: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    /// Only include rows strictly newer than this many hours ago.
    pub since_hours: Option<i64>,
}

pub async fn list_setup_rejections_filtered(
    pool: &PgPool,
    f: &RejectionFilter,
) -> Result<Vec<V2SetupRejectionRow>, StorageError> {
    let limit = f.limit.clamp(1, 5_000);
    let rows = sqlx::query_as::<_, V2SetupRejectionRow>(
        r#"SELECT id, created_at, venue_class, exchange, symbol, timeframe,
                  profile, direction, reject_reason, confluence_id, raw_meta
             FROM qtss_v2_setup_rejections
            WHERE ($1::text IS NULL OR venue_class = $1)
              AND ($2::text IS NULL OR reject_reason = $2)
              AND ($3::text IS NULL OR symbol = $3)
              AND ($4::text IS NULL OR timeframe = $4)
              AND ($5::bigint IS NULL OR created_at > now() - ($5 || ' hours')::interval)
            ORDER BY created_at DESC
            LIMIT $6"#,
    )
    .bind(f.venue_class.as_deref())
    .bind(f.reason.as_deref())
    .bind(f.symbol.as_deref())
    .bind(f.timeframe.as_deref())
    .bind(f.since_hours)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Aggregate count per reason for the Confluence Inspector summary card.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RejectionReasonCount {
    pub reject_reason: String,
    pub n: i64,
}

pub async fn summarize_setup_rejections(
    pool: &PgPool,
    since_hours: i64,
    venue_class: Option<&str>,
) -> Result<Vec<RejectionReasonCount>, StorageError> {
    let rows = sqlx::query_as::<_, RejectionReasonCount>(
        r#"SELECT reject_reason, COUNT(*)::bigint AS n
             FROM qtss_v2_setup_rejections
            WHERE created_at > now() - ($1 || ' hours')::interval
              AND ($2::text IS NULL OR venue_class = $2)
            GROUP BY reject_reason
            ORDER BY n DESC"#,
    )
    .bind(since_hours)
    .bind(venue_class)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
