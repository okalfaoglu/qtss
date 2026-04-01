//! Telegram Bot webhook — onay/red düğmeleri: `ai_decisions` → `d:{uuid}:a|r`, `ai_approval_requests` → `a:{uuid}:a|r`. Yol şifresi: `system_config.notify.telegram_webhook_secret`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use qtss_storage::SystemConfigRepository;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::state::SharedState;

pub fn telegram_webhook_router() -> Router<SharedState> {
    Router::new().route("/telegram/webhook/{secret}", post(handle_update))
}

fn parse_decision_callback(data: &str) -> Option<(Uuid, bool)> {
    parse_prefixed_callback(data, "d:")
}

fn parse_approval_request_callback(data: &str) -> Option<(Uuid, bool)> {
    parse_prefixed_callback(data, "a:")
}

fn parse_prefixed_callback(data: &str, prefix: &str) -> Option<(Uuid, bool)> {
    let rest = data.strip_prefix(prefix)?;
    let (uid_str, action) = rest.rsplit_once(':')?;
    let id = Uuid::parse_str(uid_str).ok()?;
    match action {
        "a" => Some((id, true)),
        "r" => Some((id, false)),
        _ => None,
    }
}

async fn webhook_secret_matches(pool: &sqlx::PgPool, path_secret: &str) -> bool {
    let Ok(Some(row)) = SystemConfigRepository::new(pool.clone())
        .get("notify", "telegram_webhook_secret")
        .await
    else {
        return false;
    };
    let Some(expected) = row.value.get("value").and_then(|v| v.as_str()) else {
        return false;
    };
    let exp = expected.trim();
    !exp.is_empty() && exp == path_secret.trim()
}

async fn handle_update(
    Path(secret): Path<String>,
    State(st): State<SharedState>,
    Json(update): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    if !webhook_secret_matches(&st.pool, &secret).await {
        return Err(StatusCode::NOT_FOUND);
    }

    let cq = match update.get("callback_query") {
        Some(v) if !v.is_null() => v,
        _ => return Ok(Json(json!({ "ok": true }))),
    };

    let cq_id = cq.get("id").and_then(|x| x.as_str()).unwrap_or("");
    let data = cq.get("data").and_then(|x| x.as_str()).unwrap_or("");
    let from_id = cq
        .get("from")
        .and_then(|u| u.get("id"))
        .map(|v| v.to_string())
        .unwrap_or_else(|| "0".into());

    let ncfg = qtss_ai::load_notify_config_merged(&st.pool).await;
    let disp = qtss_notify::NotificationDispatcher::new(ncfg);

    let tg_uid: i64 = from_id.parse().unwrap_or(0);

    if let Some((approval_id, approve)) = parse_approval_request_callback(data) {
        let st_norm = if approve { "approved" } else { "rejected" };
        let result = st
            .ai_approval
            .decide_pending_via_telegram(approval_id, st_norm, tg_uid)
            .await;
        match result {
            Ok(0) => {
                let _ = disp
                    .telegram_answer_callback_query(
                        cq_id,
                        Some("No pending approval (wrong id or already decided)"),
                    )
                    .await;
            }
            Ok(_) => {
                let by = format!("telegram:{from_id}");
                if let Err(e) =
                    qtss_ai::mirror_approval_request_outcome_to_linked_ai_decisions(
                        &st.pool,
                        approval_id,
                        approve,
                        &by,
                    )
                    .await
                {
                    tracing::warn!(
                        error = %e,
                        "mirror linked ai_decisions after approval_request telegram"
                    );
                }
                let msg = if approve {
                    "Approval request approved"
                } else {
                    "Approval request rejected"
                };
                let _ = disp.telegram_answer_callback_query(cq_id, Some(msg)).await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "telegram approval_request webhook");
                let _ = disp
                    .telegram_answer_callback_query(cq_id, Some("Server error"))
                    .await;
            }
        }
        return Ok(Json(json!({ "ok": true })));
    }

    let Some((decision_id, approve)) = parse_decision_callback(data) else {
        let _ = disp
            .telegram_answer_callback_query(cq_id, Some("Invalid callback"))
            .await;
        return Ok(Json(json!({ "ok": true })));
    };

    let by = format!("telegram:{from_id}");
    let result = if approve {
        qtss_ai::storage::admin_approve_ai_decision(&st.pool, decision_id, &by).await
    } else {
        qtss_ai::storage::admin_reject_ai_decision(&st.pool, decision_id, &by).await
    };

    match result {
        Ok(0) => {
            let _ = disp
                .telegram_answer_callback_query(
                    cq_id,
                    Some("No pending decision (already decided or expired)"),
                )
                .await;
        }
        Ok(_) => {
            let msg = if approve { "Approved" } else { "Rejected" };
            let _ = disp.telegram_answer_callback_query(cq_id, Some(msg)).await;
        }
        Err(e) => {
            tracing::warn!(error = %e, "telegram AI decision approve/reject webhook");
            let _ = disp
                .telegram_answer_callback_query(cq_id, Some("Server error"))
                .await;
        }
    }

    Ok(Json(json!({ "ok": true })))
}
