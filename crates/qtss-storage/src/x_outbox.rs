//! Faz 9.7.6 — `x_outbox` queue repo.
//!
//! Keeps the publisher loop small: enqueue a row on every lifecycle
//! event, let the drain loop pick pending rows in FIFO order, atomic-
//! claim them (status='sending' guard), post to X, stamp `tweet_id` +
//! `permalink`. Failures bump `attempt_count` and park the row as
//! `failed` once `max_attempts` is exceeded (caller decision).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone)]
pub struct XOutboxInsert {
    pub setup_id: Option<Uuid>,
    pub lifecycle_event_id: Option<Uuid>,
    pub event_key: String,
    pub body: String,
    pub image_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct XOutboxRow {
    pub id: Uuid,
    pub setup_id: Option<Uuid>,
    pub lifecycle_event_id: Option<Uuid>,
    pub event_key: String,
    pub body: String,
    pub image_path: Option<String>,
    pub status: String,
    pub attempt_count: i16,
    pub last_error: Option<String>,
    pub tweet_id: Option<String>,
    pub permalink: Option<String>,
    pub sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn enqueue_x_outbox(
    pool: &PgPool,
    ins: &XOutboxInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO x_outbox (setup_id, lifecycle_event_id, event_key, body, image_path)
           VALUES ($1,$2,$3,$4,$5) RETURNING id"#,
    )
    .bind(ins.setup_id)
    .bind(ins.lifecycle_event_id)
    .bind(&ins.event_key)
    .bind(&ins.body)
    .bind(&ins.image_path)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Atomically claim up to `limit` pending rows (status pending→sending).
/// The `FOR UPDATE SKIP LOCKED` pattern means parallel publishers do
/// not double-claim; only one instance runs in practice today but it
/// keeps us honest.
pub async fn claim_x_outbox_batch(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<XOutboxRow>, StorageError> {
    let rows = sqlx::query_as::<_, XOutboxRow>(
        r#"
        WITH claimed AS (
            SELECT id FROM x_outbox
             WHERE status = 'pending'
             ORDER BY created_at ASC
             LIMIT $1
             FOR UPDATE SKIP LOCKED
        )
        UPDATE x_outbox SET
            status        = 'sending',
            attempt_count = attempt_count + 1,
            updated_at    = NOW()
         WHERE id IN (SELECT id FROM claimed)
         RETURNING id, setup_id, lifecycle_event_id, event_key, body, image_path,
                   status, attempt_count, last_error, tweet_id, permalink,
                   sent_at, created_at, updated_at
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_x_sent(
    pool: &PgPool,
    id: Uuid,
    tweet_id: &str,
    permalink: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE x_outbox SET
               status     = 'sent',
               tweet_id   = $2,
               permalink  = $3,
               sent_at    = NOW(),
               updated_at = NOW()
            WHERE id = $1"#,
    )
    .bind(id)
    .bind(tweet_id)
    .bind(permalink)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_x_failed(
    pool: &PgPool,
    id: Uuid,
    error: &str,
    terminal: bool,
) -> Result<(), StorageError> {
    let new_status = if terminal { "failed" } else { "pending" };
    sqlx::query(
        r#"UPDATE x_outbox SET
               status     = $2,
               last_error = $3,
               updated_at = NOW()
            WHERE id = $1"#,
    )
    .bind(id)
    .bind(new_status)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Count rows with `status='sent'` emitted today (UTC) — used by the
/// publisher loop to enforce a daily cap.
pub async fn count_sent_today_utc(pool: &PgPool) -> Result<i64, StorageError> {
    let n = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM x_outbox
            WHERE status = 'sent'
              AND sent_at >= date_trunc('day', NOW() AT TIME ZONE 'utc')"#,
    )
    .fetch_one(pool)
    .await?;
    Ok(n)
}
