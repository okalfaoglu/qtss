//! `system_config_audit` — immutable change log for system_config (migration 0037).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SystemConfigAuditRow {
    pub id: i64,
    pub module: String,
    pub config_key: String,
    pub action: String,
    pub old_value: Option<JsonValue>,
    pub new_value: Option<JsonValue>,
    pub changed_by: Option<Uuid>,
    pub changed_at: DateTime<Utc>,
}

pub struct SystemConfigAuditRepository {
    pool: PgPool,
}

impl SystemConfigAuditRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Fetch change history for a specific config key, newest first.
    pub async fn history(
        &self,
        module: &str,
        config_key: &str,
        limit: i64,
    ) -> Result<Vec<SystemConfigAuditRow>, StorageError> {
        let lim = limit.clamp(1, 100);
        let rows = sqlx::query_as::<_, SystemConfigAuditRow>(
            r#"SELECT id, module, config_key, action, old_value, new_value, changed_by, changed_at
               FROM system_config_audit
               WHERE module = $1 AND config_key = $2
               ORDER BY changed_at DESC, id DESC
               LIMIT $3"#,
        )
        .bind(module.trim())
        .bind(config_key.trim())
        .bind(lim)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Fetch a specific audit entry by id.
    pub async fn get_by_id(&self, audit_id: i64) -> Result<Option<SystemConfigAuditRow>, StorageError> {
        let row = sqlx::query_as::<_, SystemConfigAuditRow>(
            r#"SELECT id, module, config_key, action, old_value, new_value, changed_by, changed_at
               FROM system_config_audit
               WHERE id = $1"#,
        )
        .bind(audit_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }
}
