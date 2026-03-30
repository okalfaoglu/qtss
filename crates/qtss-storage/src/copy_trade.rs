//! Copy-trade abonelikleri (`copy_subscriptions`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CopySubscriptionRow {
    pub id: Uuid,
    pub leader_user_id: Uuid,
    pub follower_user_id: Uuid,
    pub rule: serde_json::Value,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

pub struct CopySubscriptionRepository {
    pool: PgPool,
}

impl CopySubscriptionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Active copy subscriptions (follower execution / monitoring).
    pub async fn list_active_subscriptions(
        &self,
    ) -> Result<Vec<CopySubscriptionRow>, StorageError> {
        let rows = sqlx::query_as::<_, CopySubscriptionRow>(
            r#"SELECT id, leader_user_id, follower_user_id, rule, active, created_at
               FROM copy_subscriptions
               WHERE active = true
               ORDER BY created_at ASC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn list_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<CopySubscriptionRow>, StorageError> {
        let rows = sqlx::query_as::<_, CopySubscriptionRow>(
            r#"SELECT id, leader_user_id, follower_user_id, rule, active, created_at
               FROM copy_subscriptions
               WHERE leader_user_id = $1 OR follower_user_id = $1
               ORDER BY created_at DESC"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn create(
        &self,
        leader_user_id: Uuid,
        follower_user_id: Uuid,
        rule: serde_json::Value,
    ) -> Result<CopySubscriptionRow, StorageError> {
        let row = sqlx::query_as::<_, CopySubscriptionRow>(
            r#"INSERT INTO copy_subscriptions (leader_user_id, follower_user_id, rule)
               VALUES ($1, $2, $3)
               RETURNING id, leader_user_id, follower_user_id, rule, active, created_at"#,
        )
        .bind(leader_user_id)
        .bind(follower_user_id)
        .bind(rule)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn set_active_for_participant(
        &self,
        id: Uuid,
        user_id: Uuid,
        active: bool,
    ) -> Result<u64, StorageError> {
        let res = sqlx::query(
            r#"UPDATE copy_subscriptions SET active = $1
               WHERE id = $2 AND (leader_user_id = $3 OR follower_user_id = $3)"#,
        )
        .bind(active)
        .bind(id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    pub async fn delete_for_participant(
        &self,
        id: Uuid,
        user_id: Uuid,
    ) -> Result<u64, StorageError> {
        let res = sqlx::query(
            r#"DELETE FROM copy_subscriptions
               WHERE id = $1 AND (leader_user_id = $2 OR follower_user_id = $2)"#,
        )
        .bind(id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }
}
