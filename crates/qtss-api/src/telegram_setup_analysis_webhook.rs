//! Telegram Bot webhook: buffered chart/text → Gemini → Turkish report (`telegram_setup_analysis` config).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use tracing::{info, warn};

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
        warn!(
            path_secret_len = secret.trim().len(),
            expected_secret_configured = !expected.is_empty(),
            "telegram setup_analysis: webhook secret mismatch or empty (returning 404)"
        );
        return Err(StatusCode::NOT_FOUND);
    }

    let top_keys: Vec<String> = update
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let update_id = update.get("update_id").and_then(|x| x.as_i64());
    info!(
        target: "qtss_telegram_setup_analysis",
        ?update_id,
        ?top_keys,
        has_message = update.get("message").filter(|v| !v.is_null()).is_some(),
        has_channel_post = update.get("channel_post").filter(|v| !v.is_null()).is_some(),
        "telegram setup_analysis webhook: authenticated, dispatching update"
    );

    let ncfg = qtss_ai::load_notify_config_merged(&st.pool).await;
    let Some(ref tg) = ncfg.telegram else {
        warn!(
            ?update_id,
            "telegram setup_analysis: notify telegram not configured, acking update without processing"
        );
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
