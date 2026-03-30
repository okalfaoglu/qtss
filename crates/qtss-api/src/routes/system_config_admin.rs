//! Admin CRUD for `system_config` (FAZ 11.6).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Extension, Json, Router};
use qtss_common::{log_business, QtssLogLevel};
use serde::Deserialize;
use uuid::Uuid;

use qtss_storage::SystemConfigRow;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct SystemConfigListQuery {
    pub module: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct SystemConfigUpsertBody {
    pub module: String,
    pub config_key: String,
    pub value: serde_json::Value,
    pub schema_version: Option<i32>,
    pub description: Option<String>,
    pub is_secret: Option<bool>,
}

pub fn system_config_admin_router() -> Router<SharedState> {
    Router::new()
        .route(
            "/admin/system-config",
            get(list_system_config).post(upsert_system_config),
        )
        .route(
            "/admin/system-config/{module}/{key}",
            get(get_system_config).delete(delete_system_config),
        )
}

async fn list_system_config(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<SystemConfigListQuery>,
) -> Result<Json<Vec<SystemConfigRow>>, ApiError> {
    let _ = claims;
    let limit = q.limit.unwrap_or(500);
    let rows = match q.module.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(m) => st.system_config.list_by_module(m, limit).await?,
        None => st.system_config.list_all(limit).await?,
    };
    log_business(QtssLogLevel::Debug, "qtss_api::system_config", "list");
    Ok(Json(rows))
}

async fn get_system_config(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path((module, key)): Path<(String, String)>,
) -> Result<Json<SystemConfigRow>, ApiError> {
    let _ = claims;
    let row = st
        .system_config
        .get(&module, &key)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "system_config row not found"))?;
    Ok(Json(row))
}

async fn upsert_system_config(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<SystemConfigUpsertBody>,
) -> Result<Json<SystemConfigRow>, ApiError> {
    let uid = Uuid::parse_str(claims.sub.trim()).ok();
    let row = st
        .system_config
        .upsert(
            &body.module,
            &body.config_key,
            body.value,
            body.schema_version,
            body.description.as_deref(),
            body.is_secret,
            uid,
        )
        .await?;
    log_business(
        QtssLogLevel::Info,
        "qtss_api::system_config",
        format!("upsert {}.{}", row.module, row.config_key),
    );
    Ok(Json(row))
}

async fn delete_system_config(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path((module, key)): Path<(String, String)>,
) -> Result<Json<u64>, ApiError> {
    let _ = claims;
    let n = st.system_config.delete(&module, &key).await?;
    log_business(
        QtssLogLevel::Warning,
        "qtss_api::system_config",
        format!("delete {}.{} rows={}", module, key, n),
    );
    Ok(Json(n))
}
