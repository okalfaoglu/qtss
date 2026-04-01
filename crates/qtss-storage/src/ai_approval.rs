//! `ai_approval_requests` — human-in-the-loop queue for AI or policy suggestions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AiApprovalRequestRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub requester_user_id: Uuid,
    pub status: String,
    pub kind: String,
    pub payload: serde_json::Value,
    pub model_hint: Option<String>,
    pub admin_note: Option<String>,
    pub decided_by_user_id: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct AiApprovalRepository {
    pool: PgPool,
}

impl AiApprovalRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(
        &self,
        org_id: Uuid,
        requester_user_id: Uuid,
        kind: &str,
        payload: serde_json::Value,
        model_hint: Option<&str>,
    ) -> Result<AiApprovalRequestRow, StorageError> {
        let row = sqlx::query_as::<_, AiApprovalRequestRow>(
            r#"INSERT INTO ai_approval_requests (
                   org_id, requester_user_id, kind, payload, model_hint, status
               ) VALUES ($1, $2, $3, $4, $5, 'pending')
               RETURNING id, org_id, requester_user_id, status, kind, payload, model_hint,
                         admin_note, decided_by_user_id, decided_at, created_at, updated_at"#,
        )
        .bind(org_id)
        .bind(requester_user_id)
        .bind(kind)
        .bind(payload)
        .bind(model_hint)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Newest first; optional `status` filter (exact match).
    pub async fn list_for_org(
        &self,
        org_id: Uuid,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AiApprovalRequestRow>, StorageError> {
        let lim = limit.clamp(1, 200);
        let rows = if let Some(st) = status {
            sqlx::query_as::<_, AiApprovalRequestRow>(
                r#"SELECT id, org_id, requester_user_id, status, kind, payload, model_hint,
                          admin_note, decided_by_user_id, decided_at, created_at, updated_at
                   FROM ai_approval_requests
                   WHERE org_id = $1 AND status = $2
                   ORDER BY created_at DESC
                   LIMIT $3"#,
            )
            .bind(org_id)
            .bind(st)
            .bind(lim)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, AiApprovalRequestRow>(
                r#"SELECT id, org_id, requester_user_id, status, kind, payload, model_hint,
                          admin_note, decided_by_user_id, decided_at, created_at, updated_at
                   FROM ai_approval_requests
                   WHERE org_id = $1
                   ORDER BY created_at DESC
                   LIMIT $2"#,
            )
            .bind(org_id)
            .bind(lim)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows)
    }

    /// Sets terminal status for a **pending** row in `org_id`. Returns rows affected (0 if not found / wrong org / not pending).
    pub async fn fetch_by_id_for_org(
        &self,
        id: Uuid,
        org_id: Uuid,
    ) -> Result<Option<AiApprovalRequestRow>, StorageError> {
        let row = sqlx::query_as::<_, AiApprovalRequestRow>(
            r#"SELECT id, org_id, requester_user_id, status, kind, payload, model_hint,
                      admin_note, decided_by_user_id, decided_at, created_at, updated_at
               FROM ai_approval_requests
               WHERE id = $1 AND org_id = $2"#,
        )
        .bind(id)
        .bind(org_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn decide(
        &self,
        id: Uuid,
        org_id: Uuid,
        decided_by_user_id: Uuid,
        new_status: &str,
        admin_note: Option<&str>,
    ) -> Result<u64, StorageError> {
        if new_status != "approved" && new_status != "rejected" {
            return Err(StorageError::Other(
                "decide: new_status must be approved or rejected".into(),
            ));
        }
        let res = sqlx::query(
            r#"UPDATE ai_approval_requests
               SET status = $1,
                   admin_note = $2,
                   decided_by_user_id = $3,
                   decided_at = now(),
                   updated_at = now()
               WHERE id = $4 AND org_id = $5 AND status = 'pending'"#,
        )
        .bind(new_status)
        .bind(admin_note)
        .bind(decided_by_user_id)
        .bind(id)
        .bind(org_id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Telegram webhook: yalnızca **`pending`** satırı `id` ile güncellenir; `decided_by_user_id` NULL, kaynak `admin_note` içinde.
    pub async fn decide_pending_via_telegram(
        &self,
        id: Uuid,
        new_status: &str,
        telegram_user_id: i64,
    ) -> Result<u64, StorageError> {
        if new_status != "approved" && new_status != "rejected" {
            return Err(StorageError::Other(
                "decide_pending_via_telegram: new_status must be approved or rejected".into(),
            ));
        }
        let note = format!("[telegram:{telegram_user_id}]");
        let res = sqlx::query(
            r#"UPDATE ai_approval_requests
               SET status = $1,
                   admin_note = $2,
                   decided_by_user_id = NULL,
                   decided_at = now(),
                   updated_at = now()
               WHERE id = $3 AND status = 'pending'"#,
        )
        .bind(new_status)
        .bind(note)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }
}
