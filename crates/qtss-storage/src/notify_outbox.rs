//! `notify_outbox` — queued notifications for async delivery (worker + `qtss-notify`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NotifyOutboxRow {
    pub id: Uuid,
    pub org_id: Option<Uuid>,
    pub title: String,
    pub body: String,
    pub channels: Json<Vec<String>>,
    pub status: String,
    pub attempt_count: i32,
    pub last_error: Option<String>,
    pub sent_at: Option<DateTime<Utc>>,
    pub delivery_detail: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct NotifyOutboxRepository {
    pool: PgPool,
}

impl NotifyOutboxRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn enqueue(
        &self,
        org_id: Option<Uuid>,
        title: &str,
        body: &str,
        channels: Vec<String>,
    ) -> Result<NotifyOutboxRow, StorageError> {
        let ch_json = Json(channels);
        let row = sqlx::query_as::<_, NotifyOutboxRow>(
            r#"INSERT INTO notify_outbox (org_id, title, body, channels, status)
               VALUES ($1, $2, $3, $4, 'pending')
               RETURNING id, org_id, title, body, channels, status, attempt_count, last_error,
                         sent_at, delivery_detail, created_at, updated_at"#,
        )
        .bind(org_id)
        .bind(title)
        .bind(body)
        .bind(ch_json)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_recent_for_org(
        &self,
        org_id: Uuid,
        limit: i64,
    ) -> Result<Vec<NotifyOutboxRow>, StorageError> {
        let lim = limit.clamp(1, 200);
        let rows = sqlx::query_as::<_, NotifyOutboxRow>(
            r#"SELECT id, org_id, title, body, channels, status, attempt_count, last_error,
                      sent_at, delivery_detail, created_at, updated_at
               FROM notify_outbox
               WHERE org_id = $1
               ORDER BY created_at DESC
               LIMIT $2"#,
        )
        .bind(org_id)
        .bind(lim)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn claim_next_pending(&self) -> Result<Option<NotifyOutboxRow>, StorageError> {
        let mut tx = self.pool.begin().await?;
        let id: Option<(Uuid,)> = sqlx::query_as(
            r#"SELECT id FROM notify_outbox
               WHERE status = 'pending'
               ORDER BY created_at ASC
               LIMIT 1
               FOR UPDATE SKIP LOCKED"#,
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some((id,)) = id else {
            tx.commit().await?;
            return Ok(None);
        };
        let row = sqlx::query_as::<_, NotifyOutboxRow>(
            r#"UPDATE notify_outbox
               SET status = 'sending',
                   attempt_count = attempt_count + 1,
                   updated_at = now()
               WHERE id = $1
               RETURNING id, org_id, title, body, channels, status, attempt_count, last_error,
                         sent_at, delivery_detail, created_at, updated_at"#,
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(row))
    }

    pub async fn mark_sent(&self, id: Uuid, detail: Value) -> Result<(), StorageError> {
        sqlx::query(
            r#"UPDATE notify_outbox
               SET status = 'sent',
                   sent_at = now(),
                   delivery_detail = $2,
                   last_error = NULL,
                   updated_at = now()
               WHERE id = $1"#,
        )
        .bind(id)
        .bind(detail)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_failed(&self, id: Uuid, err: &str) -> Result<(), StorageError> {
        sqlx::query(
            r#"UPDATE notify_outbox
               SET status = 'failed',
                   last_error = $2,
                   updated_at = now()
               WHERE id = $1"#,
        )
        .bind(id)
        .bind(err)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
