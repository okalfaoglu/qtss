//! Bildirim kanalları — `qtss-notify` + ortam değişkenleri; kalıcı kuyruk `notify_outbox`.

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use qtss_notify::{Notification, NotificationChannel, NotificationDispatcher, NotifyError};
use qtss_storage::NotifyOutboxRow;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

pub fn notify_router() -> Router<SharedState> {
    Router::new()
        .route("/notify/test", post(notify_test))
        .route("/notify/outbox", get(list_notify_outbox))
}

/// `POST /notify/outbox` — `require_ops_roles`.
pub fn notify_outbox_write_router() -> Router<SharedState> {
    Router::new().route("/notify/outbox", post(enqueue_notify_outbox))
}

#[derive(Deserialize)]
pub struct NotifyTestBody {
    pub channel: Option<String>,
    pub message: Option<String>,
}

#[derive(Deserialize)]
pub struct ListOutboxQuery {
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct EnqueueOutboxBody {
    pub title: String,
    pub body: String,
    pub channels: Option<Vec<String>>,
}

async fn notify_test(
    Extension(_claims): Extension<AccessClaims>,
    Json(body): Json<NotifyTestBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ch = body
        .channel
        .as_deref()
        .and_then(NotificationChannel::parse)
        .unwrap_or(NotificationChannel::Telegram);
    let msg = body
        .message
        .unwrap_or_else(|| "QTSS panel — bildirim testi".into());
    let n = Notification::new("QTSS test", msg);
    let d = NotificationDispatcher::from_env();
    match d.send(ch, &n).await {
        Ok(rec) => Ok(Json(json!({
            "status": "sent",
            "receipt": rec,
        }))),
        Err(NotifyError::ChannelNotConfigured(msg)) => Err(ApiError::bad_request(format!(
            "kanal yapılandırılmadı: {msg}"
        ))),
        Err(e) => Err(ApiError::new(StatusCode::BAD_GATEWAY, e.to_string())),
    }
}

fn parse_org(claims: &AccessClaims) -> Result<Uuid, ApiError> {
    Uuid::parse_str(claims.org_id.trim()).map_err(|_| ApiError::bad_request("geçersiz token org_id"))
}

async fn list_notify_outbox(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<ListOutboxQuery>,
) -> Result<Json<Vec<NotifyOutboxRow>>, ApiError> {
    let org_id = parse_org(&claims)?;
    let limit = q.limit.unwrap_or(50);
    let rows = st.notify_outbox.list_recent_for_org(org_id, limit).await?;
    Ok(Json(rows))
}

async fn enqueue_notify_outbox(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<EnqueueOutboxBody>,
) -> Result<Json<NotifyOutboxRow>, ApiError> {
    let org_id = parse_org(&claims)?;
    let title = body.title.trim().to_string();
    let body_text = body.body.trim().to_string();
    if title.is_empty() || body_text.is_empty() {
        return Err(ApiError::bad_request("title ve body dolu olmalı"));
    }
    let mut channels = body.channels.unwrap_or_default();
    channels.retain(|s| !s.trim().is_empty());
    if channels.is_empty() {
        channels.push("webhook".to_string());
    }
    let row = st
        .notify_outbox
        .enqueue(Some(org_id), &title, &body_text, channels)
        .await?;
    Ok(Json(row))
}
