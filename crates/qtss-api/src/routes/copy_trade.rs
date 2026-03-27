//! Copy-trade abonelikleri.

use axum::extract::{Extension, Path, State};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use qtss_domain::copy_trade::CopyRule;
use qtss_storage::CopySubscriptionRow;

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
) -> Result<Json<Vec<CopySubscriptionRow>>, String> {
    let uid = Uuid::parse_str(&claims.sub).map_err(|_| "geçersiz token sub".to_string())?;
    let rows = st
        .copy
        .list_for_user(uid)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(rows))
}

async fn create_subscription(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<CreateCopySubscriptionBody>,
) -> Result<Json<CopySubscriptionRow>, String> {
    let follower = Uuid::parse_str(&claims.sub).map_err(|_| "geçersiz token sub".to_string())?;
    let _rule: CopyRule =
        serde_json::from_value(body.rule.clone()).map_err(|e| format!("rule: {e}"))?;
    let row = st
        .copy
        .create(body.leader_user_id, follower, body.rule)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(row))
}

async fn set_subscription_active(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetActiveBody>,
) -> Result<Json<serde_json::Value>, String> {
    let uid = Uuid::parse_str(&claims.sub).map_err(|_| "geçersiz token sub".to_string())?;
    let n = st
        .copy
        .set_active_for_participant(id, uid, body.active)
        .await
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("abonelik bulunamadı veya yetki yok".into());
    }
    Ok(Json(serde_json::json!({ "updated": n })))
}

async fn delete_subscription(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, String> {
    let uid = Uuid::parse_str(&claims.sub).map_err(|_| "geçersiz token sub".to_string())?;
    let n = st
        .copy
        .delete_for_participant(id, uid)
        .await
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("abonelik bulunamadı veya yetki yok".into());
    }
    Ok(Json(serde_json::json!({ "deleted": n })))
}
