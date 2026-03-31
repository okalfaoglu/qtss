//! `notify_outbox` — queued notifications for async delivery (worker + `qtss-notify`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::Json;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NotifyOutboxRow {
    pub id: Uuid,
    pub org_id: Option<Uuid>,
    pub event_key: Option<String>,
    pub severity: String,
    pub exchange: Option<String>,
    pub segment: Option<String>,
    pub symbol: Option<String>,
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
        self.enqueue_with_meta(org_id, None, "info", None, None, None, title, body, channels)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn enqueue_with_meta(
        &self,
        org_id: Option<Uuid>,
        event_key: Option<&str>,
        severity: &str,
        exchange: Option<&str>,
        segment: Option<&str>,
        symbol: Option<&str>,
        title: &str,
        body: &str,
        channels: Vec<String>,
    ) -> Result<NotifyOutboxRow, StorageError> {
        let ch_json = Json(channels);
        let row = sqlx::query_as::<_, NotifyOutboxRow>(
            r#"INSERT INTO notify_outbox (
                   org_id, event_key, severity, exchange, segment, symbol,
                   title, body, channels, status
               ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'pending')
               RETURNING id, org_id, event_key, severity, exchange, segment, symbol,
                         title, body, channels, status, attempt_count, last_error,
                         sent_at, delivery_detail, created_at, updated_at"#,
        )
        .bind(org_id)
        .bind(event_key)
        .bind(severity)
        .bind(exchange)
        .bind(segment)
        .bind(symbol)
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
        self.list_recent_for_org_filtered(org_id, None, None, None, None, None, None, limit)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list_recent_for_org_filtered(
        &self,
        org_id: Uuid,
        status: Option<&str>,
        event_key: Option<&str>,
        exchange: Option<&str>,
        segment: Option<&str>,
        symbol: Option<&str>,
        query: Option<&str>,
        limit: i64,
    ) -> Result<Vec<NotifyOutboxRow>, StorageError> {
        let lim = limit.clamp(1, 200);
        let rows = sqlx::query_as::<_, NotifyOutboxRow>(
            r#"SELECT id, org_id, event_key, severity, exchange, segment, symbol,
                      title, body, channels, status, attempt_count, last_error,
                      sent_at, delivery_detail, created_at, updated_at
               FROM notify_outbox
               WHERE org_id = $1
                 AND ($2::text IS NULL OR status = $2)
                 AND ($3::text IS NULL OR event_key = $3)
                 AND ($4::text IS NULL OR exchange = $4)
                 AND ($5::text IS NULL OR segment = $5)
                 AND ($6::text IS NULL OR symbol = $6)
                 AND (
                     $7::text IS NULL
                     OR title ILIKE ('%' || $7 || '%')
                     OR body ILIKE ('%' || $7 || '%')
                     OR COALESCE(last_error, '') ILIKE ('%' || $7 || '%')
                     OR channels::text ILIKE ('%' || $7 || '%')
                 )
               ORDER BY created_at DESC
               LIMIT $8"#,
        )
        .bind(org_id)
        .bind(status)
        .bind(event_key)
        .bind(exchange)
        .bind(segment)
        .bind(symbol)
        .bind(query)
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
               RETURNING id, org_id, event_key, severity, exchange, segment, symbol,
                         title, body, channels, status, attempt_count, last_error,
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
