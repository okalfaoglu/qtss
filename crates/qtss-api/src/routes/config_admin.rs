//! Admin config CRUD — Bearer access_token + RBAC (ileride).

use axum::extract::{Extension, Path, State};
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::Deserialize;

use uuid::Uuid;

use qtss_common::{log_business, QtssLogLevel};
use qtss_storage::AppConfigEntry;

use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct UpsertBody {
    pub key: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
    pub actor_user_id: Option<Uuid>,
}

pub fn config_router() -> Router<SharedState> {
    Router::new()
        .route("/config", get(list_config).post(upsert_config))
        .route("/config/{key}", delete(delete_config))
}

async fn list_config(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
) -> Result<Json<Vec<AppConfigEntry>>, String> {
    let _ = claims;
    let rows = st.config.list(500).await.map_err(|e| e.to_string())?;
    log_business(QtssLogLevel::Debug, "qtss_api::config", "list_config");
    Ok(Json(rows))
}

async fn upsert_config(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<UpsertBody>,
) -> Result<Json<AppConfigEntry>, String> {
    let _ = claims;
    let row = st
        .config
        .upsert(
            &body.key,
            body.value,
            body.description.as_deref(),
            body.actor_user_id,
        )
        .await
        .map_err(|e| e.to_string())?;
    log_business(
        QtssLogLevel::Info,
        "qtss_api::config",
        format!("upsert {}", row.key),
    );
    Ok(Json(row))
}

async fn delete_config(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(key): Path<String>,
) -> Result<Json<u64>, String> {
    let _ = claims;
    let n = st.config.delete_by_key(&key).await.map_err(|e| e.to_string())?;
    log_business(
        QtssLogLevel::Warning,
        "qtss_api::config",
        format!("delete {} rows={}", key, n),
    );
    Ok(Json(n))
}
