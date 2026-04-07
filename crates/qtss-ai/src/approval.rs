//! Auto-approve gate + human notification when below threshold (FAZ 4.3).

use sqlx::PgPool;
use uuid::Uuid;

use serde_json::{json, Value};

use crate::config::AiEngineConfig;
use crate::error::AiResult;
use qtss_notify::{escape_telegram_html, Notification, NotificationChannel, NotificationDispatcher};
use qtss_signal_card::{
    format_compact_price, try_render_ai_approval_card_png, try_render_operational_approval_card_png,
    AiApprovalCardInput, OperationalApprovalCardInput,
};

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
    pub operational_trailing_callback_pct: Option<f64>,
    pub operational_partial_close_pct: Option<f64>,
    pub strategic_risk_budget_pct: Option<f64>,
    pub strategic_max_open_positions: Option<i32>,
    pub strategic_preferred_regime: Option<String>,
    /// JSON string of `symbol_scores` (truncated in Telegram HTML).
    pub strategic_symbol_scores_json: Option<String>,
}

impl AiDecisionNotifySnapshot {
    #[must_use]
    pub fn from_strategic_parsed(parsed: &Value) -> Self {
        let scores_raw = parsed
            .get("symbol_scores")
            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".into()))
            .filter(|s| !s.is_empty());
        Self {
            layer: "strategic".into(),
            strategic_risk_budget_pct: parsed.get("risk_budget_pct").and_then(|v| v.as_f64()),
            strategic_max_open_positions: parsed
                .get("max_open_positions")
                .and_then(|v| v.as_i64())
                .map(|x| x as i32),
            strategic_preferred_regime: parsed
                .get("preferred_regime")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            strategic_symbol_scores_json: scores_raw,
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
        s.operational_trailing_callback_pct =
            parsed.get("trailing_callback_pct").and_then(|v| v.as_f64());
        s.operational_partial_close_pct = parsed.get("partial_close_pct").and_then(|v| v.as_f64());
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

/// Trading position side for UI: **Long** / **Short** / **Flat** (no directional trade).
/// `neutral` and `no_trade` from the LLM mean flat — not "neutral" as a fourth category.
fn direction_emoji_and_position_label(direction: Option<&str>) -> (&'static str, &'static str) {
    let d = direction
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    match d.as_deref() {
        Some("buy") | Some("long") | Some("strong_buy") => ("🟢", "Long"),
        Some("sell") | Some("short") | Some("strong_sell") => ("🔴", "Short"),
        _ => ("⚪", "Flat"),
    }
}

#[must_use]
fn notify_ui_turkish(cfg: &AiEngineConfig) -> bool {
    cfg.output_locale
        .as_deref()
        .unwrap_or("tr")
        .to_lowercase()
        .starts_with("tr")
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

fn format_confidence_bar(confidence: f64, threshold: f64, turkish_ui: bool) -> String {
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
    let thr_word = if turkish_ui { "eşik" } else { "threshold" };
    format!(
        "<code>{}</code>\n<code>{}</code>\n{}%  ·  {} {}%",
        bar,
        marker_line,
        (conf * 100.0).round() as i32,
        thr_word,
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

fn try_operational_approval_png(
    symbol: &str,
    confidence: f64,
    snapshot: &AiDecisionNotifySnapshot,
) -> Option<Vec<u8>> {
    if snapshot.layer != "operational" {
        return None;
    }
    let action = snapshot.operational_action.as_deref()?.trim();
    if action.is_empty() {
        return None;
    }
    let input = OperationalApprovalCardInput {
        symbol: symbol.trim().to_uppercase(),
        timeframe: snapshot
            .timeframe
            .clone()
            .unwrap_or_else(|| "—".into()),
        last_close: snapshot.last_price,
        approx_change_pct: snapshot.approx_price_change_pct,
        action: action.to_string(),
        confidence_0_1: confidence.clamp(0.0, 1.0),
        new_sl_pct: snapshot.operational_new_sl_pct,
        new_tp_pct: snapshot.operational_new_tp_pct,
        trailing_callback_pct: snapshot.operational_trailing_callback_pct,
        partial_close_pct: snapshot.operational_partial_close_pct,
    };
    match try_render_operational_approval_card_png(&input) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            tracing::warn!(error = %e, "operational AI approval card PNG render failed");
            None
        }
    }
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
        flat_no_trade: false,
    };
    match try_render_ai_approval_card_png(&input) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            tracing::warn!(error = %e, "AI approval card PNG render failed");
            None
        }
    }
}

/// Tactical layer, **no** long/short (`neutral` / `no_trade`): still send a FLAT card if we have a price.
fn try_tactical_flat_approval_png(
    symbol: &str,
    direction: Option<&str>,
    confidence: f64,
    snapshot: &AiDecisionNotifySnapshot,
) -> Option<Vec<u8>> {
    if snapshot.layer != "tactical" {
        return None;
    }
    if side_long_from_direction(direction).is_some() {
        return None;
    }
    let ref_px = snapshot.entry_hint.or(snapshot.last_price)?;
    if !ref_px.is_finite() || ref_px.abs() < 1e-12 {
        return None;
    }
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
        side_long: true,
        confidence_0_1: confidence.clamp(0.0, 1.0),
        reference_price: ref_px,
        stop_loss: ref_px,
        take_profit: ref_px,
        flat_no_trade: true,
    };
    match try_render_ai_approval_card_png(&input) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            tracing::warn!(error = %e, "AI flat approval card PNG render failed");
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
    let kind = match snapshot.layer.as_str() {
        "operational" => "operasyonel özet",
        "tactical" => "taktik özet",
        _ => "özet",
    };
    let line = format!(
        "{sym} ({tf}) — AI {kind} kartı. Sonraki mesajda gerekçe ve Onay/Red düğmeleri."
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
    let dir = direction_emoji_and_position_label(direction).1;
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
             new_take_profit_pct: {:?}\n\
             trailing_callback_pct: {:?}\n\
             partial_close_pct: {:?}\n",
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
            snapshot.operational_trailing_callback_pct,
            snapshot.operational_partial_close_pct,
        ));
    } else if snapshot.layer == "strategic" {
        extra.push_str(&format!(
            "\n\
             risk_budget_pct: {:?}\n\
             max_open_positions: {:?}\n\
             preferred_regime: {:?}\n",
            snapshot.strategic_risk_budget_pct,
            snapshot.strategic_max_open_positions,
            snapshot.strategic_preferred_regime,
        ));
        if let Some(ref s) = snapshot.strategic_symbol_scores_json {
            let head: String = s.chars().take(800).collect();
            extra.push_str("symbol_scores (head):\n");
            extra.push_str(&head);
            if s.len() > 800 {
                extra.push_str("…\n");
            }
        }
    }
    let thr_word = if notify_ui_turkish(cfg) {
        "eşik"
    } else {
        "threshold"
    };
    format!(
        "decision_id: {decision_id}\n\
         layer: {}\n\
         symbol: {sym}\n\
         direction: {dir}\n\
         confidence: {confidence:.4} ({thr_word} {:.2}, auto_approve: {})\
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
    let turkish_ui = notify_ui_turkish(cfg);
    let (emoji, dir_label) = direction_emoji_and_position_label(direction);
    let sym_display = symbol.unwrap_or("—");
    let sym_esc = escape_telegram_html(sym_display);
    let bar_block = format_confidence_bar(confidence, cfg.auto_approve_threshold, turkish_ui);
    let threshold_ok = confidence + f64::EPSILON >= cfg.auto_approve_threshold;
    let gate_line = if !cfg.auto_approve_enabled {
        "⚙️ Otomatik onay: <b>kapalı</b>"
    } else if threshold_ok {
        "✅ Güven eşiğe ulaşıyor (politika yine de manuel adım isteyebilir)."
    } else {
        "⚠️ Güven <b>eşiğin altında</b> — lütfen inceleyin."
    };
    let reasoning_raw = reasoning.unwrap_or(if turkish_ui {
        "(gerekçe yok)"
    } else {
        "(no reasoning text)"
    });
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
                let sl_risk_pct = ((sl_px - ref_px) / ref_px * 100.0).abs();
                let tp_reward_pct = ((tp_px - ref_px) / ref_px * 100.0).abs();
                format!(
                    "<b>Giriş (ort):</b> <code>{}</code>\n\
                     <b>Stop (SL):</b> <code>{}</code> <i>(-{:.2}%)</i>\n\
                     <b>Kar al (TP):</b> <code>{}</code> <i>(+{:.2}%)</i>",
                    escape_telegram_html(&format_compact_price(ref_px)),
                    escape_telegram_html(&format_compact_price(sl_px)),
                    sl_risk_pct,
                    escape_telegram_html(&format_compact_price(tp_px)),
                    tp_reward_pct,
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
        parts.push(format!("<b>Yön:</b> {dir_label} {emoji}"));
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
        let trail = snapshot
            .operational_trailing_callback_pct
            .map(|x| format!("{x:.4}"))
            .unwrap_or_else(|| "—".into());
        let part = snapshot
            .operational_partial_close_pct
            .map(|x| format!("{x:.4}"))
            .unwrap_or_else(|| "—".into());
        format!(
            "<b>📌 Operasyonel</b>\n\
             <b>Sembol:</b> <code>{sym_esc}</code>\n\
             <b>TF:</b> <code>{tf_esc}</code>\n\
             <b>Güncel fiyat:</b> <code>{lp_esc}</code>\n\
             <b>Eylem:</b> <code>{act_esc}</code>\n\
             <b>Yeni SL %:</b> <code>{sl}</code>  ·  <b>Yeni TP %:</b> <code>{tp}</code>\n\
             <b>Trailing %:</b> <code>{trail}</code>  ·  <b>Kısmi kapat %:</b> <code>{part}</code>\n\
             <b>Yön / not:</b> {dir_label} {emoji}"
        )
    } else if snapshot.layer == "strategic" {
        let rb = snapshot
            .strategic_risk_budget_pct
            .map(|x| format!("{x:.2}"))
            .unwrap_or_else(|| "—".into());
        let mx = snapshot
            .strategic_max_open_positions
            .map(|x| x.to_string())
            .unwrap_or_else(|| "—".into());
        let reg = snapshot
            .strategic_preferred_regime
            .as_deref()
            .unwrap_or("—");
        let reg_esc = escape_telegram_html(reg);
        let scores_raw = snapshot
            .strategic_symbol_scores_json
            .as_deref()
            .unwrap_or("{}");
        let scores_short = truncate_chars(scores_raw, 900);
        let scores_esc = escape_telegram_html(&scores_short);
        format!(
            "<b>📌 Portföy stratejisi</b>\n\
             <b>Risk bütçesi %:</b> <code>{}</code>\n\
             <b>Maks. açık pozisyon:</b> <code>{}</code>\n\
             <b>Tercih edilen rejim:</b> <code>{}</code>\n\
             <b>Sembol ağırlıkları (özet):</b>\n<pre>{}</pre>",
            escape_telegram_html(&rb),
            escape_telegram_html(&mx),
            reg_esc,
            scores_esc
        )
    } else {
        format!(
            "<b>Sembol:</b> <code>{sym_esc}</code>\n\
             <b>Yön / not:</b> {dir_label} {emoji}"
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
        let n = sqlx::query(
            r#"UPDATE ai_decisions
               SET status = 'approved', approved_at = now(), approved_by = 'auto'
               WHERE id = $1 AND status = 'pending_approval'"#,
        )
        .bind(decision_id)
        .execute(pool)
        .await?
        .rows_affected();
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
        if n > 0 {
            if let Err(e) = crate::storage::notify_ai_tactical_executor_wake(pool).await {
                tracing::warn!(%e, "notify_ai_tactical_executor_wake after auto-approve");
            }
        }
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
    let sym_short = match (symbol, snapshot.layer.as_str()) {
        (Some(s), _) => s,
        (None, "strategic") => "portfolio",
        _ => "—",
    };
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
        let (approve_lbl, reject_lbl) = if notify_ui_turkish(cfg) {
            ("Onayla", "Reddet")
        } else {
            ("Approve", "Reject")
        };
        let markup = json!({"inline_keyboard":[[
            {"text": approve_lbl, "callback_data": format!("d:{}:a", decision_id)},
            {"text": reject_lbl, "callback_data": format!("d:{}:r", decision_id)},
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
            let png = try_tactical_approval_png(sym, direction, confidence, snapshot)
                .or_else(|| try_tactical_flat_approval_png(sym, direction, confidence, snapshot))
                .or_else(|| try_operational_approval_png(sym, confidence, snapshot));
            if let Some(bytes) = png {
                let cap = photo_caption_plain(Some(sym), snapshot);
                note = note.with_telegram_photo_png(bytes, cap);
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
    use serde_json::json;

    #[test]
    fn strategic_snapshot_reads_parsed_fields() {
        let v = json!({
            "risk_budget_pct": 1.5,
            "max_open_positions": 4,
            "preferred_regime": "trend",
            "symbol_scores": {"BTCUSDT": 0.7}
        });
        let s = AiDecisionNotifySnapshot::from_strategic_parsed(&v);
        assert_eq!(s.layer, "strategic");
        assert!((s.strategic_risk_budget_pct.unwrap() - 1.5).abs() < 1e-9);
        assert_eq!(s.strategic_max_open_positions, Some(4));
        assert!(s.strategic_symbol_scores_json.as_deref().unwrap().contains("BTCUSDT"));
    }

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
        assert!(s.contains("Short"));
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

    #[test]
    fn tactical_flat_png_none_without_price() {
        let snap = AiDecisionNotifySnapshot {
            layer: "tactical".into(),
            last_price: None,
            entry_hint: None,
            ..Default::default()
        };
        assert!(try_tactical_flat_approval_png("BTC", Some("neutral"), 0.5, &snap).is_none());
    }
}
