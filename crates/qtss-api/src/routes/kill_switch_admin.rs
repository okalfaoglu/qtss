//! `POST /admin/kill-switch/reset` — admin: `app_config` + yerel süreç halt temizliği; worker `kill_switch_db_sync_loop` ile senkron.

use axum::extract::{Extension, State};
use axum::routing::post;
use axum::{Json, Router};
use qtss_common::resume_trading;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

const KILL_SWITCH_APP_CONFIG_KEY: &str = "kill_switch_trading_halted";

#[derive(Deserialize)]
pub struct KillSwitchResetBody {
    pub confirm: bool,
}

pub fn kill_switch_admin_router() -> Router<SharedState> {
    Router::new().route("/admin/kill-switch/reset", post(kill_switch_reset))
}

async fn kill_switch_reset(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<KillSwitchResetBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !body.confirm {
        return Err(ApiError::bad_request("confirm: true gerekli"));
    }
    let uid = Uuid::parse_str(claims.sub.trim())
        .map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    st.config
        .upsert(
            KILL_SWITCH_APP_CONFIG_KEY,
            json!(false),
            Some("Trading halt — false allows strategies; worker syncs via kill_switch_db_sync_loop"),
            Some(uid),
        )
        .await?;
    resume_trading();
    Ok(Json(json!({
        "status": "ok",
        "kill_switch_trading_halted": false,
        "note": "Worker süreçleri app_config değerini birkaç saniye içinde okuyup halt kaldırır"
    })))
}
