//! Authenticated status for Telegram setup-analysis (no secrets).

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::state::SharedState;

pub fn telegram_setup_analysis_status_router() -> Router<SharedState> {
    Router::new().route("/telegram-setup-analysis/status", get(get_status))
}

async fn get_status(State(st): State<SharedState>) -> Json<Value> {
    let cfg = qtss_telegram_setup_analysis::ResolvedSetupAnalysisConfig::load(&st.pool).await;
    Json(json!({
        "webhook_configured": cfg.webhook_enabled(),
        "gemini_configured": cfg.gemini_configured(),
        "trigger_phrase": cfg.trigger_phrase,
        "gemini_model": cfg.gemini_model,
        "max_buffer_turns": cfg.max_buffer_turns,
        "buffer_ttl_secs": cfg.buffer_ttl_secs,
        "allowlist_restricts": cfg.allowlist_restricts(),
        "allowlist_size": cfg.allowlist_size(),
        "webhook_path": "/telegram/setup-analysis/{secret}",
    }))
}
