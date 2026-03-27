//! HTTP denetim satırları.

use chrono::{DateTime, Utc};
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
}

pub async fn insert_http_audit(pool: &PgPool, row: AuditHttpRow) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO audit_log (request_id, user_id, org_id, method, path, status_code, roles)
           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
    )
    .bind(&row.request_id)
    .bind(row.user_id)
    .bind(row.org_id)
    .bind(&row.method)
    .bind(&row.path)
    .bind(row.status_code as i16)
    .bind(&row.roles)
    .execute(pool)
    .await?;
    Ok(())
}

/// Son kayıtlar (yönetim / debug).
pub async fn list_recent(pool: &PgPool, limit: i64) -> Result<Vec<AuditHttpListRow>, StorageError> {
    let rows = sqlx::query_as::<_, AuditHttpListRow>(
        r#"SELECT id, created_at, request_id, user_id, org_id, method, path, status_code, roles
           FROM audit_log ORDER BY created_at DESC LIMIT $1"#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Clone, sqlx::FromRow)]
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
}
