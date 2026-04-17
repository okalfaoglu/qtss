//! Faz 9.7.8 — Setup broadcast outbox CRUD.
//!
//! Sibling of `x_outbox` but dedicated to new-setup PublicCard
//! dispatch. Producers (the setup engine) call [`enqueue`] on insert;
//! the publisher worker claims via `FOR UPDATE SKIP LOCKED` and marks
//! [`mark_sent`] on success or [`mark_failed`] on error.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SetupBroadcastRow {
    pub id: Uuid,
    pub setup_id: Uuid,
    pub status: String,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub telegram_sent_at: Option<DateTime<Utc>>,
    pub x_enqueued_at: Option<DateTime<Utc>>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Idempotent enqueue — safe to call multiple times for the same setup.
pub async fn enqueue(pool: &PgPool, setup_id: Uuid) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        INSERT INTO setup_broadcast_outbox (setup_id, status)
             VALUES ($1, 'pending')
        ON CONFLICT (setup_id) DO NOTHING
        "#,
    )
    .bind(setup_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Atomically claim up to `limit` pending rows. Uses `FOR UPDATE SKIP
/// LOCKED` so parallel workers don't double-send. Rows flip to
/// `status='claimed'` with `claimed_at=now()`.
pub async fn claim_batch(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<SetupBroadcastRow>, StorageError> {
    let rows = sqlx::query_as::<_, SetupBroadcastRow>(
        r#"
        WITH picked AS (
            SELECT id
              FROM setup_broadcast_outbox
             WHERE status = 'pending'
             ORDER BY created_at
             LIMIT $1
             FOR UPDATE SKIP LOCKED
        )
        UPDATE setup_broadcast_outbox AS o
           SET status     = 'claimed',
               claimed_at = NOW(),
               attempts   = o.attempts + 1,
               updated_at = NOW()
          FROM picked
         WHERE o.id = picked.id
        RETURNING o.id, o.setup_id, o.status, o.attempts, o.last_error,
                  o.telegram_sent_at, o.x_enqueued_at, o.claimed_at,
                  o.sent_at, o.created_at, o.updated_at
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_sent(
    pool: &PgPool,
    id: Uuid,
    telegram_sent: bool,
    x_enqueued: bool,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        UPDATE setup_broadcast_outbox
           SET status           = 'sent',
               sent_at          = NOW(),
               telegram_sent_at = CASE WHEN $2 THEN NOW() ELSE telegram_sent_at END,
               x_enqueued_at    = CASE WHEN $3 THEN NOW() ELSE x_enqueued_at END,
               last_error       = NULL,
               updated_at       = NOW()
         WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(telegram_sent)
    .bind(x_enqueued)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark failed — if `attempts >= max_attempts`, terminal `failed`;
/// otherwise revert to `pending` for retry.
pub async fn mark_failed(
    pool: &PgPool,
    id: Uuid,
    error: &str,
    max_attempts: i32,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        UPDATE setup_broadcast_outbox
           SET status     = CASE WHEN attempts >= $3 THEN 'failed' ELSE 'pending' END,
               last_error = $2,
               updated_at = NOW()
         WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(error)
    .bind(max_attempts)
    .execute(pool)
    .await?;
    Ok(())
}
