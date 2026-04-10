#![allow(dead_code)]
//! `GET /v2/config` -- Faz 5 Adim (i).
//!
//! Read-only projection of `system_config` for the GUI Config Editor.
//! Mutations stay on the existing `/admin/system-config` admin route
//! so the role boundary stays clean: dashboard roles browse the
//! catalogue, admins edit it.
//!
//! **History & rollback** (migration 0037):
//! - `GET  /v2/config/:module/:key/history` — audit trail (dashboard roles)
//! - `POST /v2/config/:module/:key/rollback` — restore old value (admin only)

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use qtss_common::{log_business, QtssLogLevel};
use qtss_gui_api::{group_config_entries, ConfigEditorView, ConfigEntry};
use qtss_storage::{SystemConfigAuditRow, SystemConfigRow};

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

// ─── List ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ConfigQuery {
    pub module: Option<String>,
    pub limit: Option<i64>,
}

pub fn v2_config_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/config", get(get_config))
        .route("/v2/config/{module}/{key}/history", get(get_history))
}

/// Rollback requires admin role — mounted separately.
pub fn v2_config_admin_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/config/{module}/{key}/rollback", post(post_rollback))
}

async fn get_config(
    State(st): State<SharedState>,
    Query(q): Query<ConfigQuery>,
) -> Result<Json<ConfigEditorView>, ApiError> {
    let limit = q
        .limit
        .unwrap_or_else(|| env_int("QTSS_V2_CONFIG_LIMIT", 500))
        .clamp(1, 500);

    let module = q.module.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let rows: Vec<SystemConfigRow> = match module {
        Some(m) => st.system_config.list_by_module(m, limit).await?,
        None => st.system_config.list_all(limit).await?,
    };

    let entries: Vec<ConfigEntry> = rows.into_iter().map(row_to_entry).collect();
    let groups = group_config_entries(entries);

    Ok(Json(ConfigEditorView {
        generated_at: Utc::now(),
        groups,
    }))
}

// ─── History ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
}

async fn get_history(
    State(st): State<SharedState>,
    Path((module, key)): Path<(String, String)>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<SystemConfigAuditRow>>, ApiError> {
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let rows = st.system_config_audit.history(&module, &key, limit).await?;
    Ok(Json(rows))
}

// ─── Rollback ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RollbackBody {
    /// The `system_config_audit.id` whose `old_value` (or `new_value`) to restore.
    pub audit_id: i64,
    /// Which snapshot to restore: `"old"` (default) or `"new"`.
    pub snapshot: Option<String>,
}

async fn post_rollback(
    axum::Extension(claims): axum::Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path((module, key)): Path<(String, String)>,
    Json(body): Json<RollbackBody>,
) -> Result<Json<SystemConfigRow>, ApiError> {
    let uid = Uuid::parse_str(claims.sub.trim()).ok();

    let audit_row = st
        .system_config_audit
        .get_by_id(body.audit_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "audit entry not found"))?;

    // Verify the audit entry belongs to the requested key.
    if audit_row.module != module || audit_row.config_key != key {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "audit entry does not match the requested config key",
        ));
    }

    let use_new = body.snapshot.as_deref() == Some("new");
    let restore_value = if use_new {
        audit_row.new_value
    } else {
        audit_row.old_value
    };
    let restore_value = restore_value.ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "selected snapshot is null (cannot restore)",
        )
    })?;

    let row = st
        .system_config
        .upsert(&module, &key, restore_value, None, None, None, uid)
        .await?;

    log_business(
        QtssLogLevel::Info,
        "qtss_api::v2_config",
        format!("rollback {}.{} to audit_id={}", module, key, body.audit_id),
    );

    Ok(Json(row))
}

// ─── Helpers ────────────────────────────────────────────────────────────

fn row_to_entry(r: SystemConfigRow) -> ConfigEntry {
    let masked = ConfigEntry::detect_masked(&r.value, r.is_secret);
    ConfigEntry {
        module: r.module,
        config_key: r.config_key,
        value: r.value,
        schema_version: r.schema_version,
        description: r.description,
        is_secret: r.is_secret,
        masked,
        updated_at: r.updated_at,
    }
}

fn env_int(key: &str, default: i64) -> i64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    fn raw(module: &str, key: &str, secret: bool, value: serde_json::Value) -> SystemConfigRow {
        SystemConfigRow {
            id: Uuid::new_v4(),
            module: module.into(),
            config_key: key.into(),
            value,
            schema_version: 1,
            description: None,
            is_secret: secret,
            updated_at: Utc::now(),
            updated_by_user_id: None,
        }
    }

    #[test]
    fn row_to_entry_marks_masked_secrets() {
        let e = row_to_entry(raw("api", "jwt_secret", true, json!({ "_masked": true })));
        assert!(e.is_secret);
        assert!(e.masked);
    }

    #[test]
    fn query_parses() {
        let q: ConfigQuery = serde_urlencoded::from_str("module=api&limit=100").unwrap();
        assert_eq!(q.module.as_deref(), Some("api"));
        assert_eq!(q.limit, Some(100));
    }
}
