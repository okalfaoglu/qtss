//! Telegram Bot webhook: buffered chart/text → Gemini → Turkish report (`telegram_setup_analysis` config).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use tracing::warn;

use crate::state::SharedState;

pub fn telegram_setup_analysis_router() -> Router<SharedState> {
    Router::new().route("/telegram/setup-analysis/{secret}", post(handle_update))
}

async fn handle_update(
    Path(secret): Path<String>,
    State(st): State<SharedState>,
    Json(update): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let cfg = qtss_telegram_setup_analysis::ResolvedSetupAnalysisConfig::load(&st.pool).await;
    let expected = cfg.webhook_secret.trim();
    if expected.is_empty() || expected != secret.trim() {
        return Err(StatusCode::NOT_FOUND);
    }

    let ncfg = qtss_ai::load_notify_config_merged(&st.pool).await;
    let Some(ref tg) = ncfg.telegram else {
        warn!("telegram setup_analysis: notify telegram not configured, ignoring update");
        return Ok(Json(json!({ "ok": true })));
    };

    qtss_telegram_setup_analysis::process_telegram_update(
        &st.http_client,
        &st.setup_analysis_buffers,
        &update,
        &cfg,
        tg.bot_token.trim(),
    )
    .await;

    Ok(Json(json!({ "ok": true })))
}
