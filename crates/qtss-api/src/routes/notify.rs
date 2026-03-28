//! Bildirim kanalları — `qtss-notify` + ortam değişkenleri.

use axum::extract::Extension;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use qtss_notify::{Notification, NotificationChannel, NotificationDispatcher, NotifyError};
use serde::Deserialize;
use serde_json::json;

use crate::oauth::AccessClaims;
use crate::state::SharedState;

pub fn notify_router() -> Router<SharedState> {
    Router::new().route("/notify/test", post(notify_test))
}

#[derive(Deserialize)]
pub struct NotifyTestBody {
    pub channel: Option<String>,
    pub message: Option<String>,
}

async fn notify_test(
    Extension(_claims): Extension<AccessClaims>,
    Json(body): Json<NotifyTestBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
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
        Err(NotifyError::ChannelNotConfigured(msg)) => Err((
            StatusCode::BAD_REQUEST,
            format!("kanal yapılandırılmadı: {msg}"),
        )),
        Err(e) => Err((StatusCode::BAD_GATEWAY, e.to_string())),
    }
}
