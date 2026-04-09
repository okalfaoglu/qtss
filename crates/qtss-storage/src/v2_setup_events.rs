//! Faz 8.0 — `qtss_v2_setup_events` repo. Outbox for Telegram and
//! tracing consumers. Events are inserted with `delivery_state =
//! 'pending'`; a consumer flips to `delivered`/`failed`/`skipped`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct V2SetupEventRow {
    pub id: Uuid,
    pub setup_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub event_type: String,
    pub payload: JsonValue,
    pub delivery_state: String,
    pub delivered_at: Option<DateTime<Utc>>,
    pub retries: i32,
}

#[derive(Debug, Clone)]
pub struct V2SetupEventInsert {
    pub setup_id: Uuid,
    pub event_type: String,
    pub payload: JsonValue,
}

pub async fn insert_v2_setup_event(
    pool: &PgPool,
    row: &V2SetupEventInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO qtss_v2_setup_events (setup_id, event_type, payload)
        VALUES ($1, $2, $3)
        RETURNING id
        "#,
    )
    .bind(row.setup_id)
    .bind(&row.event_type)
    .bind(&row.payload)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn list_pending_setup_events(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<V2SetupEventRow>, StorageError> {
    let rows = sqlx::query_as::<_, V2SetupEventRow>(
        r#"SELECT id, setup_id, created_at, event_type, payload,
                  delivery_state, delivered_at, retries
             FROM qtss_v2_setup_events
            WHERE delivery_state = 'pending'
            ORDER BY created_at ASC
            LIMIT $1"#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_setup_event_delivered(
    pool: &PgPool,
    id: Uuid,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE qtss_v2_setup_events
              SET delivery_state = 'delivered',
                  delivered_at = now()
            WHERE id = $1"#,
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_setup_event_failed(
    pool: &PgPool,
    id: Uuid,
    retries: i32,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE qtss_v2_setup_events
              SET delivery_state = 'failed',
                  retries = $2
            WHERE id = $1"#,
    )
    .bind(id)
    .bind(retries)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_events_for_setup(
    pool: &PgPool,
    setup_id: Uuid,
) -> Result<Vec<V2SetupEventRow>, StorageError> {
    let rows = sqlx::query_as::<_, V2SetupEventRow>(
        r#"SELECT id, setup_id, created_at, event_type, payload,
                  delivery_state, delivered_at, retries
             FROM qtss_v2_setup_events
            WHERE setup_id = $1
            ORDER BY created_at ASC"#,
    )
    .bind(setup_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
