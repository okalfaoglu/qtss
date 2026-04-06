//! Auto-approve gate + human notification when below threshold (FAZ 4.3).

use sqlx::PgPool;
use uuid::Uuid;

use serde_json::{json, Value};

use crate::config::AiEngineConfig;
use crate::error::AiResult;
use qtss_notify::{escape_telegram_html, Notification, NotificationChannel, NotificationDispatcher};
use qtss_signal_card::{format_compact_price, try_render_ai_approval_card_png, AiApprovalCardInput};

/// Price / timeframe context for Telegram + optional PNG card (built from LLM JSON + bar context).
#[derive(Clone, Debug, Default)]
pub struct AiDecisionNotifySnapshot {
    /// `tactical` | `operational` | `strategic` | empty.
    pub layer: String,
    pub timeframe: Option<String>,
    pub last_price: Option<f64>,
    pub approx_price_change_pct: Option<f64>,
    pub entry_hint: Option<f64>,
    pub stop_loss_pct: Option<f64>,
    pub take_profit_pct: Option<f64>,
    pub operational_action: Option<String>,
    pub operational_new_sl_pct: Option<f64>,
    pub operational_new_tp_pct: Option<f64>,
}

impl AiDecisionNotifySnapshot {
    #[must_use]
    pub fn strategic_portfolio() -> Self {
        Self {
            layer: "strategic".into(),
            ..Default::default()
        }
    }

    #[must_use]
    pub fn from_tactical_context(ctx: &Value, parsed: &Value) -> Self {
        let mut s = Self {
            layer: "tactical".into(),
            ..Default::default()
        };
        if let Some(pc) = ctx.get("price_context").and_then(|x| x.as_object()) {
            s.timeframe = pc
                .get("interval")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            s.last_price = pc.get("last_close").and_then(|v| v.as_f64());
            s.approx_price_change_pct = pc
                .get("approx_change_over_window_pct")
                .and_then(|v| v.as_f64());
        }
        s.entry_hint = parsed.get("entry_price_hint").and_then(|v| v.as_f64());
        s.stop_loss_pct = parsed.get("stop_loss_pct").and_then(|v| v.as_f64());
        s.take_profit_pct = parsed.get("take_profit_pct").and_then(|v| v.as_f64());
        s
    }

    #[must_use]
    pub fn from_operational_context(ctx: &Value, parsed: &Value) -> Self {
        let mut s = Self {
            layer: "operational".into(),
            ..Default::default()
        };
        if let Some(rs) = ctx.get("recent_price_stats").and_then(|x| x.as_object()) {
            s.timeframe = rs
                .get("interval")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            s.last_price = rs.get("last_close").and_then(|v| v.as_f64());
            s.approx_price_change_pct = rs
                .get("approx_change_over_window_pct")
                .and_then(|v| v.as_f64());
        }
        s.operational_action = parsed
            .get("action")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        s.operational_new_sl_pct = parsed.get("new_stop_loss_pct").and_then(|v| v.as_f64());
        s.operational_new_tp_pct = parsed.get("new_take_profit_pct").and_then(|v| v.as_f64());
        s
    }
}

/// Pure auto-approve gate (unit-tested; same rule as [`maybe_auto_approve`] DB branch).
#[must_use]
pub fn auto_approve_eligible(confidence: f64, cfg: &AiEngineConfig) -> bool {
    cfg.auto_approve_enabled && confidence + f64::EPSILON >= cfg.auto_approve_threshold
}

const TELEGRAM_REASONING_MAX_CHARS: usize = 2800;
const TELEGRAM_PHOTO_CAPTION_MAX: usize = 900;

fn direction_emoji_and_label(direction: Option<&str>) -> (&'static str, &'static str) {
    let d = direction
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    match d.as_deref() {
        Some("buy") | Some("long") | Some("strong_buy") => ("🟢", "Long"),
        Some("sell") | Some("short") | Some("strong_sell") => ("🔴", "Short"),
        Some("neutral") => ("⚪", "Neutral"),
        _ => ("⚪", "—"),
    }
}

/// `Some(true)` long, `Some(false)` short, `None` if not directional.
fn side_long_from_direction(direction: Option<&str>) -> Option<bool> {
    let d = direction
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    match d.as_deref() {
        Some("buy") | Some("long") | Some("strong_buy") => Some(true),
        Some("sell") | Some("short") | Some("strong_sell") => Some(false),
        _ => None,
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

fn tactical_price_levels(
    snapshot: &AiDecisionNotifySnapshot,
    side_long: bool,
) -> Option<(f64, f64, f64)> {
    let ref_px = snapshot.entry_hint.or(snapshot.last_price)?;
    if !ref_px.is_finite() || ref_px.abs() < 1e-12 {
        return None;
    }
    let sl_pct = snapshot.stop_loss_pct?;
    let tp_pct = snapshot.take_profit_pct?;
    let m_sl = sl_pct / 100.0;
    let m_tp = tp_pct / 100.0;
    let (sl_px, tp_px) = if side_long {
        (ref_px * (1.0 - m_sl), ref_px * (1.0 + m_tp))
    } else {
        (ref_px * (1.0 + m_sl), ref_px * (1.0 - m_tp))
    };
    if !sl_px.is_finite() || !tp_px.is_finite() {
        return None;
    }
    Some((ref_px, sl_px, tp_px))
}

fn try_tactical_approval_png(
    symbol: &str,
    direction: Option<&str>,
    confidence: f64,
    snapshot: &AiDecisionNotifySnapshot,
) -> Option<Vec<u8>> {
    if snapshot.layer != "tactical" {
        return None;
    }
    let side = side_long_from_direction(direction)?;
    let (ref_px, sl_px, tp_px) = tactical_price_levels(snapshot, side)?;
    let last = snapshot.last_price.unwrap_or(ref_px);
    let tf = snapshot
        .timeframe
        .clone()
        .unwrap_or_else(|| "—".into());
    let input = AiApprovalCardInput {
        symbol: symbol.trim().to_uppercase(),
        timeframe: tf,
        last_close: last,
        approx_change_pct: snapshot.approx_price_change_pct,
        side_long: side,
        confidence_0_1: confidence.clamp(0.0, 1.0),
        reference_price: ref_px,
        stop_loss: sl_px,
        take_profit: tp_px,
    };
    match try_render_ai_approval_card_png(&input) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            tracing::warn!(error = %e, "AI approval card PNG render failed");
            None
        }
    }
}

fn photo_caption_plain(symbol: Option<&str>, snapshot: &AiDecisionNotifySnapshot) -> String {
    let sym = symbol.unwrap_or("—");
    let tf = snapshot
        .timeframe
        .as_deref()
        .unwrap_or("—");
    let line = format!(
        "{sym} ({tf}) — AI özet kartı. Sonraki mesajda gerekçe ve Onay/Red düğmeleri."
    );
    truncate_chars(&line, TELEGRAM_PHOTO_CAPTION_MAX)
}

fn build_plain_operator_body(
    decision_id: Uuid,
    symbol: Option<&str>,
    direction: Option<&str>,
    confidence: f64,
    cfg: &AiEngineConfig,
    reasoning: Option<&str>,
    snapshot: &AiDecisionNotifySnapshot,
) -> String {
    let sym = symbol.unwrap_or("(unknown)");
    let dir = direction.unwrap_or("(unknown)");
    let reason = reasoning.unwrap_or("(none)");
    let tf = snapshot
        .timeframe
        .as_deref()
        .unwrap_or("(n/a)");
    let mut extra = String::new();
    if snapshot.layer == "tactical" {
        if let Some(side) = side_long_from_direction(direction) {
            if let Some((ref_px, sl_px, tp_px)) = tactical_price_levels(snapshot, side) {
                let lp = snapshot
                    .last_price
                    .map(|p| format_compact_price(p))
                    .unwrap_or_else(|| "—".into());
                extra.push_str(&format!(
                    "\n\
                     timeframe: {tf}\n\
                     last_price: {lp}\n\
                     reference_entry: {}\n\
                     stop_loss: {}\n\
                     take_profit: {}\n",
                    format_compact_price(ref_px),
                    format_compact_price(sl_px),
                    format_compact_price(tp_px),
                ));
            }
        }
    } else if snapshot.layer == "operational" {
        extra.push_str(&format!(
            "\n\
             timeframe: {tf}\n\
             last_price: {}\n\
             operational_action: {}\n\
             new_stop_loss_pct: {:?}\n\
             new_take_profit_pct: {:?}\n",
            snapshot
                .last_price
                .map(format_compact_price)
                .unwrap_or_else(|| "—".into()),
            snapshot
                .operational_action
                .as_deref()
                .unwrap_or("(n/a)"),
            snapshot.operational_new_sl_pct,
            snapshot.operational_new_tp_pct,
        ));
    }
    format!(
        "decision_id: {decision_id}\n\
         layer: {}\n\
         symbol: {sym}\n\
         direction: {dir}\n\
         confidence: {confidence:.4} (threshold {:.2}, auto_approve: {})\
         {extra}\n\
         reasoning:\n{reason}",
        snapshot.layer,
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
    snapshot: &AiDecisionNotifySnapshot,
) -> String {
    let (emoji, dir_en) = direction_emoji_and_label(direction);
    let sym_display = symbol.unwrap_or("—");
    let sym_esc = escape_telegram_html(sym_display);
    let bar_block = format_confidence_bar(confidence, cfg.auto_approve_threshold);
    let threshold_ok = confidence + f64::EPSILON >= cfg.auto_approve_threshold;
    let gate_line = if !cfg.auto_approve_enabled {
        "⚙️ Otomatik onay: <b>kapalı</b>"
    } else if threshold_ok {
        "✅ Güven eşiğe ulaşıyor (politika yine de manuel adım isteyebilir)."
    } else {
        "⚠️ Güven <b>eşiğin altında</b> — lütfen inceleyin."
    };
    let reasoning_raw = reasoning.unwrap_or("(no reasoning text)");
    let reasoning_cut = truncate_chars(reasoning_raw, TELEGRAM_REASONING_MAX_CHARS);
    let reasoning_esc = escape_telegram_html(&reasoning_cut);

    let tf_esc = escape_telegram_html(
        snapshot
            .timeframe
            .as_deref()
            .unwrap_or("—"),
    );

    let setup_block = if snapshot.layer == "tactical" {
        let lp_line = snapshot.last_price.map(|p| {
            let chg = snapshot
                .approx_price_change_pct
                .map(|c| format!(" ({c:+.2}%)"))
                .unwrap_or_default();
            format!(
                "<b>Güncel fiyat:</b> <code>{}</code>{chg}",
                escape_telegram_html(&format_compact_price(p))
            )
        });
        let levels = side_long_from_direction(direction).and_then(|side| {
            tactical_price_levels(snapshot, side).map(|(ref_px, sl_px, tp_px)| {
                let sl_pct = (sl_px - ref_px) / ref_px * 100.0;
                let tp_pct = (tp_px - ref_px) / ref_px * 100.0;
                format!(
                    "<b>Giriş (ort):</b> <code>{}</code>\n\
                     <b>Stop (SL):</b> <code>{}</code> <i>({:+.2}%)</i>\n\
                     <b>Kar al (TP):</b> <code>{}</code> <i>({:+.2}%)</i>",
                    escape_telegram_html(&format_compact_price(ref_px)),
                    escape_telegram_html(&format_compact_price(sl_px)),
                    sl_pct,
                    escape_telegram_html(&format_compact_price(tp_px)),
                    tp_pct,
                )
            })
        });
        let mut parts: Vec<String> = vec![
            "<b>📌 Kurulum</b>".into(),
            format!("<b>Sembol:</b> <code>{sym_esc}</code>"),
            format!("<b>Zaman dilimi (TF):</b> <code>{tf_esc}</code>"),
        ];
        if let Some(l) = lp_line {
            parts.push(l);
        }
        parts.push(format!("<b>Yön:</b> {dir_en} {emoji}"));
        if let Some(lv) = levels {
            parts.push(lv);
        }
        parts.join("\n")
    } else if snapshot.layer == "operational" {
        let lp = snapshot.last_price.map(format_compact_price).unwrap_or_else(|| "—".into());
        let lp_esc = escape_telegram_html(&lp);
        let act = snapshot
            .operational_action
            .as_deref()
            .unwrap_or("—");
        let act_esc = escape_telegram_html(act);
        let sl = snapshot
            .operational_new_sl_pct
            .map(|x| format!("{x:.4}"))
            .unwrap_or_else(|| "—".into());
        let tp = snapshot
            .operational_new_tp_pct
            .map(|x| format!("{x:.4}"))
            .unwrap_or_else(|| "—".into());
        format!(
            "<b>📌 Operasyonel</b>\n\
             <b>Sembol:</b> <code>{sym_esc}</code>\n\
             <b>TF:</b> <code>{tf_esc}</code>\n\
             <b>Güncel fiyat:</b> <code>{lp_esc}</code>\n\
             <b>Eylem:</b> <code>{act_esc}</code>\n\
             <b>Yeni SL %:</b> <code>{sl}</code>  ·  <b>Yeni TP %:</b> <code>{tp}</code>\n\
             <b>Yön / not:</b> {dir_en} {emoji}"
        )
    } else {
        format!(
            "<b>Sembol:</b> <code>{sym_esc}</code>\n\
             <b>Yön / not:</b> {dir_en} {emoji}"
        )
    };

    format!(
        "{emoji} <b>AI karar — onay bekleniyor</b>\n\
         \n\
         {setup_block}\n\
         \n\
         <b>Güven</b>\n\
         {bar_block}\n\
         {gate_line}\n\
         \n\
         <b>Yorum / gerekçe</b>\n\
         {reasoning_esc}\n\
         \n\
         <i>Karar kimliği</i> · <code>{decision_id}</code>"
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
    snapshot: &AiDecisionNotifySnapshot,
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
        snapshot,
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
            snapshot,
        );
        let mut note = Notification::new(title, body)
            .with_telegram_html_message(tg_html)
            .with_telegram_reply_markup(markup);
        if let Some(sym) = symbol {
            if let Some(png) = try_tactical_approval_png(sym, direction, confidence, snapshot) {
                let cap = photo_caption_plain(Some(sym), snapshot);
                note = note.with_telegram_photo_png(png, cap);
            }
        }
        note
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
        let snap = AiDecisionNotifySnapshot::default();
        let s = build_plain_operator_body(
            Uuid::nil(),
            Some("BTCUSDT"),
            Some("sell"),
            0.65,
            &cfg,
            Some("hello"),
            &snap,
        );
        assert!(!s.contains("Some("));
        assert!(s.contains("BTCUSDT"));
        assert!(s.contains("sell"));
    }

    #[test]
    fn telegram_html_escapes_reasoning() {
        let cfg = AiEngineConfig::default_disabled();
        let snap = AiDecisionNotifySnapshot::default();
        let html = build_telegram_html_body(
            Uuid::nil(),
            Some("ETHUSDT"),
            Some("buy"),
            0.6,
            &cfg,
            Some("a < b & c"),
            &snap,
        );
        assert!(html.contains("&lt;"));
        assert!(html.contains("&amp;"));
        assert!(!html.contains("Some("));
    }

    #[test]
    fn tactical_png_skipped_without_take_profit_pct() {
        let snap_missing_tp = AiDecisionNotifySnapshot {
            layer: "tactical".into(),
            timeframe: Some("15m".into()),
            last_price: Some(100.0),
            stop_loss_pct: Some(2.0),
            take_profit_pct: None,
            ..Default::default()
        };
        assert!(try_tactical_approval_png("BTC", Some("buy"), 0.7, &snap_missing_tp).is_none());
    }
}
