//! `system_config` — module-scoped JSON settings (FAZ 11.4).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SystemConfigRow {
    pub id: Uuid,
    pub module: String,
    pub config_key: String,
    pub value: JsonValue,
    pub schema_version: i32,
    pub description: Option<String>,
    pub is_secret: bool,
    pub updated_at: DateTime<Utc>,
    pub updated_by_user_id: Option<Uuid>,
}

pub struct SystemConfigRepository {
    pool: PgPool,
}

impl SystemConfigRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn mask_if_secret(mut row: SystemConfigRow) -> SystemConfigRow {
        if row.is_secret {
            row.value = serde_json::json!({ "_masked": true });
        }
        row
    }

    pub async fn list_by_module(
        &self,
        module: &str,
        limit: i64,
    ) -> Result<Vec<SystemConfigRow>, StorageError> {
        let lim = limit.clamp(1, 500);
        let rows = sqlx::query_as::<_, SystemConfigRow>(
            r#"SELECT id, module, config_key, value, schema_version, description, is_secret,
                      updated_at, updated_by_user_id
               FROM system_config WHERE module = $1 ORDER BY config_key ASC LIMIT $2"#,
        )
        .bind(module.trim())
        .bind(lim)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Self::mask_if_secret).collect())
    }

    pub async fn list_all(&self, limit: i64) -> Result<Vec<SystemConfigRow>, StorageError> {
        let lim = limit.clamp(1, 1000);
        let rows = sqlx::query_as::<_, SystemConfigRow>(
            r#"SELECT id, module, config_key, value, schema_version, description, is_secret,
                      updated_at, updated_by_user_id
               FROM system_config ORDER BY module ASC, config_key ASC LIMIT $1"#,
        )
        .bind(lim)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Self::mask_if_secret).collect())
    }

    /// Single-row read (admin); **does not** mask `is_secret` so values can be edited intentionally.
    pub async fn get(
        &self,
        module: &str,
        config_key: &str,
    ) -> Result<Option<SystemConfigRow>, StorageError> {
        let row = sqlx::query_as::<_, SystemConfigRow>(
            r#"SELECT id, module, config_key, value, schema_version, description, is_secret,
                      updated_at, updated_by_user_id
               FROM system_config WHERE module = $1 AND config_key = $2"#,
        )
        .bind(module.trim())
        .bind(config_key.trim())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn get_value_json(
        pool: &PgPool,
        module: &str,
        config_key: &str,
    ) -> Result<Option<JsonValue>, StorageError> {
        let v: Option<JsonValue> = sqlx::query_scalar(
            r#"SELECT value FROM system_config WHERE module = $1 AND config_key = $2"#,
        )
        .bind(module.trim())
        .bind(config_key.trim())
        .fetch_optional(pool)
        .await?;
        Ok(v)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert(
        &self,
        module: &str,
        config_key: &str,
        value: JsonValue,
        schema_version: Option<i32>,
        description: Option<&str>,
        is_secret: Option<bool>,
        user_id: Option<Uuid>,
    ) -> Result<SystemConfigRow, StorageError> {
        let sv = schema_version.unwrap_or(1);
        let sec = is_secret.unwrap_or(false);
        let row = sqlx::query_as::<_, SystemConfigRow>(
            r#"
            INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret, updated_by_user_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (module, config_key) DO UPDATE SET
              value = EXCLUDED.value,
              schema_version = EXCLUDED.schema_version,
              description = COALESCE(EXCLUDED.description, system_config.description),
              is_secret = EXCLUDED.is_secret,
              updated_by_user_id = EXCLUDED.updated_by_user_id,
              updated_at = now()
            RETURNING id, module, config_key, value, schema_version, description, is_secret,
                      updated_at, updated_by_user_id
            "#,
        )
        .bind(module.trim())
        .bind(config_key.trim())
        .bind(value)
        .bind(sv)
        .bind(description)
        .bind(sec)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(Self::mask_if_secret(row))
    }

    pub async fn delete(&self, module: &str, config_key: &str) -> Result<u64, StorageError> {
        let res = sqlx::query("DELETE FROM system_config WHERE module = $1 AND config_key = $2")
            .bind(module.trim())
            .bind(config_key.trim())
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }
}

/// When `QTSS_CONFIG_ENV_OVERRIDES=1`, callers may prefer env over DB (FAZ 11.5).
#[must_use]
pub fn config_env_overrides_enabled() -> bool {
    matches!(
        std::env::var("QTSS_CONFIG_ENV_OVERRIDES")
            .map(|s| s.trim().to_lowercase()),
        Ok(s) if matches!(s.as_str(), "1" | "true" | "yes" | "on")
    )
}
