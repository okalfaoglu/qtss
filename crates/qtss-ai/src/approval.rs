//! Auto-approve gate + human notification when below threshold (FAZ 4.3).

use sqlx::PgPool;
use uuid::Uuid;

use crate::config::AiEngineConfig;
use crate::error::AiResult;
use qtss_notify::{Notification, NotificationChannel, NotificationDispatcher};

/// Pure auto-approve gate (unit-tested; same rule as [`maybe_auto_approve`] DB branch).
#[must_use]
pub fn auto_approve_eligible(confidence: f64, cfg: &AiEngineConfig) -> bool {
    cfg.auto_approve_enabled && confidence + f64::EPSILON >= cfg.auto_approve_threshold
}

/// If `auto_approve_enabled` and `confidence >= threshold`, marks parent + tactical children approved.
/// Otherwise sends optional Telegram/webhook via `qtss-notify` (best-effort).
pub async fn maybe_auto_approve(
    pool: &PgPool,
    decision_id: Uuid,
    confidence: f64,
    cfg: &AiEngineConfig,
    dispatcher: Option<&NotificationDispatcher>,
    symbol: Option<&str>,
    direction: Option<&str>,
    reasoning: Option<&str>,
) -> AiResult<()> {
    let approve = auto_approve_eligible(confidence, cfg);
    if approve {
        sqlx::query(
            r#"UPDATE ai_decisions
               SET status = 'approved', approved_at = now(), approved_by = 'auto'
               WHERE id = $1 AND status = 'pending_approval'"#,
        )
        .bind(decision_id)
        .execute(pool)
        .await?;
        sqlx::query(
            r#"UPDATE ai_tactical_decisions
               SET status = 'approved'
               WHERE decision_id = $1 AND status = 'pending_approval'"#,
        )
        .bind(decision_id)
        .execute(pool)
        .await?;
        sqlx::query(
            r#"UPDATE ai_position_directives
               SET status = 'approved'
               WHERE decision_id = $1 AND status = 'pending_approval'"#,
        )
        .bind(decision_id)
        .execute(pool)
        .await?;
        return Ok(());
    }

    let Some(d) = dispatcher else {
        return Ok(());
    };
    let mut channels = Vec::new();
    if d.config().telegram.is_some() {
        channels.push(NotificationChannel::Telegram);
    }
    if d.config().webhook.is_some() {
        channels.push(NotificationChannel::Webhook);
    }
    if channels.is_empty() {
        return Ok(());
    }
    let title = format!(
        "AI decision pending approval {}",
        symbol.unwrap_or("-")
    );
    let body = format!(
        "symbol={:?} direction={:?} confidence={:.4} (threshold {:.2} auto={})\nreasoning={:?}",
        symbol,
        direction,
        confidence,
        cfg.auto_approve_threshold,
        cfg.auto_approve_enabled,
        reasoning
    );
    let n = Notification::new(title, body);
    let receipts = d.send_all(&channels, &n).await;
    for r in receipts {
        if !r.ok {
            tracing::warn!(
                channel = ?r.channel,
                detail = ?r.detail,
                "AI pending decision notify channel failed"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_approve_requires_enabled_and_threshold() {
        let mut cfg = AiEngineConfig::default_disabled();
        cfg.auto_approve_enabled = false;
        cfg.auto_approve_threshold = 0.85;
        assert!(!auto_approve_eligible(0.99, &cfg));

        cfg.auto_approve_enabled = true;
        assert!(!auto_approve_eligible(0.84, &cfg));
        assert!(auto_approve_eligible(0.85, &cfg));
        assert!(auto_approve_eligible(0.851, &cfg));
    }
}
