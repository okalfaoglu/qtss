//! `copy_trade_execution_jobs` — leader `exchange_orders` fill → follower yürütme kuyruğu.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CopyTradeJobRow {
    pub id: Uuid,
    pub subscription_id: Uuid,
    pub leader_exchange_order_id: Uuid,
    pub follower_user_id: Uuid,
    pub leader_user_id: Uuid,
    pub payload: serde_json::Value,
    pub status: String,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct CopyTradeJobRepository {
    pool: PgPool,
}

impl CopyTradeJobRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// `true` if a new row was inserted.
    pub async fn try_enqueue(
        &self,
        subscription_id: Uuid,
        leader_exchange_order_id: Uuid,
        follower_user_id: Uuid,
        leader_user_id: Uuid,
        payload: serde_json::Value,
    ) -> Result<bool, StorageError> {
        let res = sqlx::query(
            r#"INSERT INTO copy_trade_execution_jobs (
                   subscription_id, leader_exchange_order_id,
                   follower_user_id, leader_user_id, payload, status
               ) VALUES ($1, $2, $3, $4, $5, 'pending')
               ON CONFLICT (subscription_id, leader_exchange_order_id) DO NOTHING"#,
        )
        .bind(subscription_id)
        .bind(leader_exchange_order_id)
        .bind(follower_user_id)
        .bind(leader_user_id)
        .bind(payload)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn claim_next_pending(&self) -> Result<Option<CopyTradeJobRow>, StorageError> {
        let mut tx = self.pool.begin().await?;
        let id: Option<(Uuid,)> = sqlx::query_as(
            r#"SELECT id FROM copy_trade_execution_jobs
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
        let row = sqlx::query_as::<_, CopyTradeJobRow>(
            r#"UPDATE copy_trade_execution_jobs
               SET status = 'processing', updated_at = now()
               WHERE id = $1
               RETURNING id, subscription_id, leader_exchange_order_id, follower_user_id,
                         leader_user_id, payload, status, error, created_at, updated_at"#,
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(row))
    }

    pub async fn mark_done(&self, id: Uuid) -> Result<(), StorageError> {
        sqlx::query(
            r#"UPDATE copy_trade_execution_jobs
               SET status = 'done', error = NULL, updated_at = now()
               WHERE id = $1"#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_skipped(&self, id: Uuid, note: &str) -> Result<(), StorageError> {
        sqlx::query(
            r#"UPDATE copy_trade_execution_jobs
               SET status = 'skipped', error = $2, updated_at = now()
               WHERE id = $1"#,
        )
        .bind(id)
        .bind(note)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_failed(&self, id: Uuid, err: &str) -> Result<(), StorageError> {
        sqlx::query(
            r#"UPDATE copy_trade_execution_jobs
               SET status = 'failed', error = $2, updated_at = now()
               WHERE id = $1"#,
        )
        .bind(id)
        .bind(err)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
