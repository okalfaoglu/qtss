//! HTTP denetim satırları.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone)]
pub struct AuditHttpRow {
    pub request_id: Option<String>,
    pub user_id: Option<Uuid>,
    pub org_id: Option<Uuid>,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub roles: Vec<String>,
    pub details: Option<Value>,
}

pub async fn insert_http_audit(pool: &PgPool, row: AuditHttpRow) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO audit_log (request_id, user_id, org_id, method, path, status_code, roles, details)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
    )
    .bind(&row.request_id)
    .bind(row.user_id)
    .bind(row.org_id)
    .bind(&row.method)
    .bind(&row.path)
    .bind(row.status_code as i16)
    .bind(&row.roles)
    .bind(&row.details)
    .execute(pool)
    .await?;
    Ok(())
}

/// Son kayıtlar (yönetim / debug). `details_kind` doluysa yalnız `details->>'kind'` eşleşen satırlar.
pub async fn list_recent(
    pool: &PgPool,
    limit: i64,
    details_kind: Option<&str>,
) -> Result<Vec<AuditHttpListRow>, StorageError> {
    let lim = limit.clamp(1, 500);
    let rows = sqlx::query_as::<_, AuditHttpListRow>(
        r#"SELECT id, created_at, request_id, user_id, org_id, method, path, status_code, roles, details
           FROM audit_log
           WHERE ($2::text IS NULL OR details->>'kind' = $2)
           ORDER BY created_at DESC
           LIMIT $1"#,
    )
    .bind(lim)
    .bind(details_kind)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AuditHttpListRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub request_id: Option<String>,
    pub user_id: Option<Uuid>,
    pub org_id: Option<Uuid>,
    pub method: String,
    pub path: String,
    pub status_code: i16,
    pub roles: Vec<String>,
    pub details: Option<Value>,
}
