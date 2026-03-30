//! Copy-trade abonelikleri.

use axum::extract::{Extension, Path, State};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use qtss_domain::copy_trade::CopyRule;
use qtss_storage::CopySubscriptionRow;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct CreateCopySubscriptionBody {
    pub leader_user_id: Uuid,
    pub rule: serde_json::Value,
}

#[derive(Deserialize)]
pub struct SetActiveBody {
    pub active: bool,
}

/// Salt okunur liste: dashboard rolleri.
pub fn copy_trade_read_router() -> Router<SharedState> {
    Router::new().route("/copy-trade/subscriptions", get(list_subscriptions))
}

/// Oluşturma / güncelleme / silme: admin veya trader.
pub fn copy_trade_write_router() -> Router<SharedState> {
    Router::new()
        .route("/copy-trade/subscriptions", post(create_subscription))
        .route(
            "/copy-trade/subscriptions/{id}/active",
            patch(set_subscription_active),
        )
        .route(
            "/copy-trade/subscriptions/{id}",
            delete(delete_subscription),
        )
}

async fn list_subscriptions(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
) -> Result<Json<Vec<CopySubscriptionRow>>, ApiError> {
    let uid = Uuid::parse_str(claims.sub.trim()).map_err(|_| {
        ApiError::bad_request("invalid token subject").with_error_key("session.invalid_sub")
    })?;
    let rows = st.copy.list_for_user(uid).await?;
    Ok(Json(rows))
}

async fn create_subscription(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<CreateCopySubscriptionBody>,
) -> Result<Json<CopySubscriptionRow>, ApiError> {
    let follower = Uuid::parse_str(claims.sub.trim()).map_err(|_| {
        ApiError::bad_request("invalid token subject").with_error_key("session.invalid_sub")
    })?;
    let _rule: CopyRule = serde_json::from_value(body.rule.clone())
        .map_err(|e| ApiError::bad_request(format!("rule: {e}")))?;
    let row = st
        .copy
        .create(body.leader_user_id, follower, body.rule)
        .await?;
    Ok(Json(row))
}

async fn set_subscription_active(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetActiveBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let uid = Uuid::parse_str(claims.sub.trim()).map_err(|_| {
        ApiError::bad_request("invalid token subject").with_error_key("session.invalid_sub")
    })?;
    let n = st
        .copy
        .set_active_for_participant(id, uid, body.active)
        .await?;
    if n == 0 {
        return Err(ApiError::not_found("abonelik bulunamadı veya yetki yok"));
    }
    Ok(Json(serde_json::json!({ "updated": n })))
}

async fn delete_subscription(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let uid = Uuid::parse_str(claims.sub.trim()).map_err(|_| {
        ApiError::bad_request("invalid token subject").with_error_key("session.invalid_sub")
    })?;
    let n = st.copy.delete_for_participant(id, uid).await?;
    if n == 0 {
        return Err(ApiError::not_found("abonelik bulunamadı veya yetki yok"));
    }
    Ok(Json(serde_json::json!({ "deleted": n })))
}
