//! Bildirim kanalları — iskelet.

use axum::extract::Extension;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::oauth::AccessClaims;
use crate::state::SharedState;

pub fn notify_router() -> Router<SharedState> {
    Router::new().route("/notify/test", post(notify_test_stub))
}

#[derive(Deserialize)]
pub struct NotifyTestBody {
    pub channel: Option<String>,
    pub message: Option<String>,
}

async fn notify_test_stub(
    Extension(_claims): Extension<AccessClaims>,
    Json(body): Json<NotifyTestBody>,
) -> Json<serde_json::Value> {
    Json(json!({
        "status": "stub",
        "channel": body.channel.unwrap_or_else(|| "telegram".into()),
        "message_preview": body.message.unwrap_or_default(),
        "queued": false,
        "detail": "qtss-notify bağlanınca kuyruğa alınacak"
    }))
}
