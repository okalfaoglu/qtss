#![allow(dead_code)]
//! `GET /v2/config` -- Faz 5 Adim (i).
//!
//! Read-only projection of `system_config` for the GUI Config Editor.
//! Mutations stay on the existing `/admin/system-config` admin route
//! so the role boundary stays clean: dashboard roles browse the
//! catalogue, admins edit it.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;

use qtss_gui_api::{group_config_entries, ConfigEditorView, ConfigEntry};
use qtss_storage::SystemConfigRow;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ConfigQuery {
    /// Optional module filter -- when set, only that module's rows
    /// are returned (still grouped, just one group).
    pub module: Option<String>,
    /// Hard cap on rows fetched from storage. Falls back to env, then
    /// to 500. The repository itself clamps at 500 server-side.
    pub limit: Option<i64>,
}

pub fn v2_config_router() -> Router<SharedState> {
    Router::new().route("/v2/config", get(get_config))
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
