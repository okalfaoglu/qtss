//! `app_config` tablosu — kodda statik config yok; admin UI ile CRUD.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppConfigEntry {
    pub id: Uuid,
    pub key: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub updated_by_user_id: Option<Uuid>,
}

pub struct AppConfigRepository {
    pool: PgPool,
}

impl AppConfigRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_by_key(&self, key: &str) -> Result<Option<AppConfigEntry>, StorageError> {
        let row = sqlx::query_as::<_, AppConfigEntry>(
            r#"SELECT id, key, value, description, updated_at, updated_by_user_id
               FROM app_config WHERE key = $1"#,
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list(&self, limit: i64) -> Result<Vec<AppConfigEntry>, StorageError> {
        let rows = sqlx::query_as::<_, AppConfigEntry>(
            r#"SELECT id, key, value, description, updated_at, updated_by_user_id
               FROM app_config ORDER BY key ASC LIMIT $1"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn upsert(
        &self,
        key: &str,
        value: serde_json::Value,
        description: Option<&str>,
        user_id: Option<Uuid>,
    ) -> Result<AppConfigEntry, StorageError> {
        let row = sqlx::query_as::<_, AppConfigEntry>(
            r#"
            INSERT INTO app_config (key, value, description, updated_by_user_id)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (key) DO UPDATE SET
              value = EXCLUDED.value,
              description = COALESCE(EXCLUDED.description, app_config.description),
              updated_by_user_id = EXCLUDED.updated_by_user_id,
              updated_at = now()
            RETURNING id, key, value, description, updated_at, updated_by_user_id
            "#,
        )
        .bind(key)
        .bind(value)
        .bind(description)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn delete_by_key(&self, key: &str) -> Result<u64, StorageError> {
        let res = sqlx::query("DELETE FROM app_config WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }

    /// `app.mode` gibi kritik anahtarlar için tip güvenli okuma.
    pub async fn get_string(&self, key: &str) -> Result<Option<String>, StorageError> {
        let Some(e) = self.get_by_key(key).await? else {
            return Ok(None);
        };
        match e.value {
            serde_json::Value::String(s) => Ok(Some(s)),
            serde_json::Value::Null => Ok(None),
            other => Ok(Some(other.to_string())),
        }
    }
}
