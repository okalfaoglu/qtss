//! Auto-approve gate + human notification when below threshold (FAZ 4.3).

use sqlx::PgPool;
use uuid::Uuid;

use serde_json::json;

use crate::config::AiEngineConfig;
use crate::error::AiResult;
use qtss_notify::{escape_telegram_html, Notification, NotificationChannel, NotificationDispatcher};

/// Pure auto-approve gate (unit-tested; same rule as [`maybe_auto_approve`] DB branch).
#[must_use]
pub fn auto_approve_eligible(confidence: f64, cfg: &AiEngineConfig) -> bool {
    cfg.auto_approve_enabled && confidence + f64::EPSILON >= cfg.auto_approve_threshold
}

const TELEGRAM_REASONING_MAX_CHARS: usize = 3200;

fn direction_emoji_and_label(direction: Option<&str>) -> (&'static str, &'static str) {
    let d = direction
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    match d.as_deref() {
        Some("buy") | Some("long") => ("🟢", "Buy"),
        Some("sell") | Some("short") => ("🔴", "Sell"),
        _ => ("⚪", "—"),
    }
}

fn format_confidence_bar(confidence: f64, threshold: f64) -> String {
    const W: usize = 16;
    let conf = confidence.clamp(0.0, 1.0);
    let thr = threshold.clamp(0.0, 1.0);
    let filled = ((conf * W as f64).round() as usize).min(W);
    let bar = "█".repeat(filled) + &"░".repeat(W - filled);
    let thr_idx = ((thr * W as f64).round() as usize).min(W.saturating_sub(1));
    let mut marker = vec!['·'; W];
    if thr_idx < W {
        marker[thr_idx] = '▲';
    }
    let marker_line: String = marker.into_iter().collect();
    format!(
        "<code>{}</code>\n<code>{}</code>\n{}%  ·  threshold {}%",
        bar,
        marker_line,
        (conf * 100.0).round() as i32,
        (thr * 100.0).round() as i32
    )
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let n = s.chars().count();
    if n <= max_chars {
        return s.to_string();
    }
    let take = max_chars.saturating_sub(1);
    s.chars().take(take).chain(std::iter::once('…')).collect()
}

fn build_plain_operator_body(
    decision_id: Uuid,
    symbol: Option<&str>,
    direction: Option<&str>,
    confidence: f64,
    cfg: &AiEngineConfig,
    reasoning: Option<&str>,
) -> String {
    let sym = symbol.unwrap_or("(unknown)");
    let dir = direction.unwrap_or("(unknown)");
    let reason = reasoning.unwrap_or("(none)");
    format!(
        "decision_id: {decision_id}\n\
         symbol: {sym}\n\
         direction: {dir}\n\
         confidence: {confidence:.4} (threshold {:.2}, auto_approve: {})\n\
         \n\
         reasoning:\n{reason}",
        cfg.auto_approve_threshold,
        if cfg.auto_approve_enabled { "on" } else { "off" },
    )
}

fn build_telegram_html_body(
    decision_id: Uuid,
    symbol: Option<&str>,
    direction: Option<&str>,
    confidence: f64,
    cfg: &AiEngineConfig,
    reasoning: Option<&str>,
) -> String {
    let (emoji, dir_en) = direction_emoji_and_label(direction);
    let sym_display = symbol.unwrap_or("—");
    let sym_esc = escape_telegram_html(sym_display);
    let bar_block = format_confidence_bar(confidence, cfg.auto_approve_threshold);
    let threshold_ok = confidence + f64::EPSILON >= cfg.auto_approve_threshold;
    let gate_line = if !cfg.auto_approve_enabled {
        "⚙️ Auto-approve: <b>off</b> (manual review always for this path)"
    } else if threshold_ok {
        "✅ Confidence is at or above threshold (still pending if policy requires manual step)."
    } else {
        "⚠️ Confidence is <b>below</b> threshold — please review."
    };
    let reasoning_raw = reasoning.unwrap_or("(no reasoning text)");
    let reasoning_cut = truncate_chars(reasoning_raw, TELEGRAM_REASONING_MAX_CHARS);
    let reasoning_esc = escape_telegram_html(&reasoning_cut);

    format!(
        "{emoji} <b>AI decision — approval needed</b>\n\
         <b>Symbol:</b> <code>{sym_esc}</code>\n\
         <b>Side:</b> {dir_en} {emoji}\n\
         \n\
         <b>Confidence gauge</b>\n\
         {bar_block}\n\
         {gate_line}\n\
         \n\
         <b>Reasoning</b>\n\
         {reasoning_esc}\n\
         \n\
         <i>Decision id</i> · <code>{decision_id}</code>"
    )
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
        crate::storage::sync_linked_approval_request_status(
            pool,
            decision_id,
            "approved",
            Some("auto"),
            None,
        )
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
    let sym_short = symbol.unwrap_or("—");
    let title = format!("AI decision pending approval · {sym_short}");
    let body = build_plain_operator_body(
        decision_id,
        symbol,
        direction,
        confidence,
        cfg,
        reasoning,
    );
    let n = if d.config().telegram.is_some() {
        let markup = json!({"inline_keyboard":[[
            {"text": "Approve", "callback_data": format!("d:{}:a", decision_id)},
            {"text": "Reject", "callback_data": format!("d:{}:r", decision_id)},
        ]]});
        let tg_html = build_telegram_html_body(
            decision_id,
            symbol,
            direction,
            confidence,
            cfg,
            reasoning,
        );
        Notification::new(title, body)
            .with_telegram_html_message(tg_html)
            .with_telegram_reply_markup(markup)
    } else {
        Notification::new(title, body)
    };
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

    #[test]
    fn plain_body_has_no_debug_option_syntax() {
        let cfg = AiEngineConfig::default_disabled();
        let s = build_plain_operator_body(
            Uuid::nil(),
            Some("BTCUSDT"),
            Some("sell"),
            0.65,
            &cfg,
            Some("hello"),
        );
        assert!(!s.contains("Some("));
        assert!(s.contains("BTCUSDT"));
        assert!(s.contains("sell"));
    }

    #[test]
    fn telegram_html_escapes_reasoning() {
        let cfg = AiEngineConfig::default_disabled();
        let html = build_telegram_html_body(
            Uuid::nil(),
            Some("ETHUSDT"),
            Some("buy"),
            0.6,
            &cfg,
            Some("a < b & c"),
        );
        assert!(html.contains("&lt;"));
        assert!(html.contains("&amp;"));
        assert!(!html.contains("Some("));
    }
}
