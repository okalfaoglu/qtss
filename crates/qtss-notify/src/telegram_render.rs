//! Faz 9.7.5 — Telegram renderer.
//!
//! Pure functions that convert a [`PublicCard`] (new-setup broadcast)
//! or a [`LifecycleContext`] (lifecycle event) into a [`Notification`]
//! with an HTML-formatted Telegram body. No I/O, no config reads —
//! the caller is expected to have loaded thresholds once and threaded
//! them through. This keeps the renderer trivially testable.
//!
//! CLAUDE.md #1 — per-kind rendering goes through dispatch tables
//! (`header_for_kind`, `emoji_for_band`) rather than scattered match
//! arms embedded in the format string.

use crate::card::{PublicCard, SetupDirection, TargetPoint};
use crate::health::HealthBand;
use crate::lifecycle::{LifecycleContext, LifecycleEventKind};
use crate::telegram_html::escape_telegram_html;
use crate::types::Notification;

// ---------------------------------------------------------------------------
// Public card (new setup broadcast)
// ---------------------------------------------------------------------------

/// Render a freshly-published setup. The returned [`Notification`]
/// carries a Turkish HTML body that Telegram parses with `parse_mode=HTML`.
/// Non-Telegram channels fall back to the plain-text `title`/`body`.
pub fn render_public_card(card: &PublicCard) -> Notification {
    let tier_emoji = emoji_for_tier(card.tier.out_of_ten);
    let title = format!(
        "{} {} {} — {}",
        tier_emoji,
        card.symbol,
        card.direction.label_tr(),
        card.pattern_label,
    );

    let mut html = String::with_capacity(512);
    html.push_str(&format!(
        "<b>{}</b> {} <b>{}</b>\n",
        escape_telegram_html(tier_emoji),
        escape_telegram_html(&card.symbol),
        escape_telegram_html(card.direction.label_tr()),
    ));
    html.push_str(&format!(
        "<i>{} · {} · {}</i>\n",
        escape_telegram_html(&card.pattern_label),
        escape_telegram_html(&card.timeframe),
        escape_telegram_html(&card.category_label),
    ));
    html.push_str(&format!(
        "Skor: <b>{}/10</b>  {}\n\n",
        card.tier.out_of_ten,
        escape_telegram_html(&card.tier.bar),
    ));

    html.push_str(&format!(
        "Giriş: <code>{}</code>\n",
        fmt_decimal(card.entry_price),
    ));
    html.push_str(&format!(
        "Stop: <code>{}</code> ({})\n",
        fmt_decimal(card.stop_price),
        fmt_pct_signed(card.stop_pct),
    ));
    for t in &card.targets {
        html.push_str(&format!(
            "TP{}: <code>{}</code> ({})\n",
            t.index,
            fmt_decimal(t.price),
            fmt_pct_signed(t.pct),
        ));
    }
    if let Some(rr) = card.risk_reward {
        html.push_str(&format!("R:R <b>{:.2}</b>\n", rr));
    }
    if let (Some(cp), Some(chg)) = (card.current_price, card.current_change_pct) {
        html.push_str(&format!(
            "\nŞu an: <code>{}</code> ({})",
            fmt_decimal(cp),
            fmt_pct_signed(chg),
        ));
    }

    let body_plain = html_to_plain(&html);
    Notification::new(title, body_plain).with_telegram_html_message(html)
}

// ---------------------------------------------------------------------------
// Lifecycle events
// ---------------------------------------------------------------------------

/// Render a lifecycle event (entry touch, TP hit, SL hit, ratchet, …)
/// for Telegram. Falls back to a short plain body for non-Telegram
/// channels (email/webhook).
pub fn render_lifecycle(ctx: &LifecycleContext) -> Notification {
    let (emoji, headline) = header_for_kind(ctx.kind, ctx.tp_index);
    let title = format!(
        "{} {} {} — {}",
        emoji,
        ctx.symbol,
        ctx.direction.label_tr(),
        headline,
    );

    let mut html = String::with_capacity(384);
    html.push_str(&format!(
        "<b>{} {}</b> — {}\n",
        escape_telegram_html(emoji),
        escape_telegram_html(&headline),
        escape_telegram_html(&ctx.symbol),
    ));
    html.push_str(&format!(
        "<i>{} · Giriş {}</i>\n",
        escape_telegram_html(ctx.direction.label_tr()),
        fmt_decimal(ctx.entry_price),
    ));
    html.push_str(&format!(
        "Fiyat: <code>{}</code>\n",
        fmt_decimal(ctx.price),
    ));
    if let Some(pnl) = ctx.pnl_pct {
        html.push_str(&format!("PnL: <b>{}</b>\n", fmt_pct_signed(pnl)));
    }
    if let Some(r) = ctx.pnl_r {
        html.push_str(&format!("R: <b>{:.2}</b>\n", r));
    }
    if let Some(h) = &ctx.health {
        html.push_str(&format!(
            "Sağlık: <b>{:.0}</b> {} {}\n",
            h.total,
            emoji_for_band(h.band),
            label_for_band(h.band),
        ));
    }
    if let Some(ai) = &ctx.ai_action {
        html.push_str(&format!("AI: <b>{}</b>", escape_telegram_html(ai)));
        if let Some(conf) = ctx.ai_confidence {
            html.push_str(&format!(" (<i>{:.0}%</i>)", conf * 100.0));
        }
        html.push('\n');
        if let Some(reason) = &ctx.ai_reasoning {
            html.push_str(&format!(
                "<i>{}</i>\n",
                escape_telegram_html(reason),
            ));
        }
    }
    if let Some(ms) = ctx.duration_ms {
        if ms > 0 {
            html.push_str(&format!("Süre: <i>{}</i>\n", fmt_duration_ms(ms)));
        }
    }

    let body_plain = html_to_plain(&html);
    Notification::new(title, body_plain).with_telegram_html_message(html)
}

// ---------------------------------------------------------------------------
// Kind / band dispatch tables (CLAUDE.md #1)
// ---------------------------------------------------------------------------

fn header_for_kind(kind: LifecycleEventKind, tp_index: Option<u8>) -> (&'static str, String) {
    match kind {
        LifecycleEventKind::EntryTouched => ("🎯", "Giriş bölgesi tetiklendi".into()),
        LifecycleEventKind::TpHit => (
            "✅",
            match tp_index {
                Some(i) => format!("TP{i} vuruldu"),
                None => "Hedef vuruldu".into(),
            },
        ),
        LifecycleEventKind::TpPartial => (
            "🟢",
            match tp_index {
                Some(i) => format!("TP{i} — kısmi kâr"),
                None => "Kısmi kâr".into(),
            },
        ),
        LifecycleEventKind::TpFinal => ("🏁", "Son hedef — pozisyon kapandı".into()),
        LifecycleEventKind::SlHit => ("🛑", "Stop tetiklendi".into()),
        LifecycleEventKind::SlRatcheted => ("🔒", "Stop yukarı çekildi".into()),
        LifecycleEventKind::Invalidated => ("❌", "Setup geçersiz".into()),
        LifecycleEventKind::Cancelled => ("↩️", "Setup iptal".into()),
        LifecycleEventKind::HealthWarn => ("⚠️", "Sağlık uyarı seviyesinde".into()),
        LifecycleEventKind::HealthDanger => ("🚨", "Sağlık tehlike seviyesinde".into()),
    }
}

fn emoji_for_tier(out_of_ten: u8) -> &'static str {
    match out_of_ten {
        9..=10 => "🏆",
        7..=8 => "⭐",
        5..=6 => "🔹",
        _ => "🔸",
    }
}

fn emoji_for_band(band: HealthBand) -> &'static str {
    match band {
        HealthBand::Healthy => "🟢",
        HealthBand::Warn => "🟡",
        HealthBand::Danger => "🟠",
        HealthBand::Critical => "🔴",
    }
}

fn label_for_band(band: HealthBand) -> &'static str {
    match band {
        HealthBand::Healthy => "Sağlıklı",
        HealthBand::Warn => "Uyarı",
        HealthBand::Danger => "Tehlike",
        HealthBand::Critical => "Kritik",
    }
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

fn fmt_decimal(d: rust_decimal::Decimal) -> String {
    // Keep up to 8 fractional digits but strip trailing zeros so crypto
    // and equities both render cleanly.
    let s = d.normalize().to_string();
    if s.is_empty() { "0".into() } else { s }
}

fn fmt_pct_signed(pct: f64) -> String {
    if pct >= 0.0 {
        format!("+{:.2}%", pct)
    } else {
        format!("{:.2}%", pct)
    }
}

fn fmt_duration_ms(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}d {}s", secs / 60, secs % 60)
    } else if secs < 86_400 {
        format!("{}s {}d", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}g {}s", secs / 86_400, (secs % 86_400) / 3600)
    }
}

/// Very lightweight HTML → plain-text: strips tags only. Adequate for
/// email/webhook fallback bodies; never used for security-sensitive
/// output.
fn html_to_plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

// Direction label bridge — used only by SHORT/LONG rendering above. We
// avoid adding a `Display` impl to keep the API flag-free.
impl SetupDirection {
    // Intentionally empty — `label_tr` already lives in `card::builder`.
}

// Silence unused-import warnings when tests are disabled.
#[allow(dead_code)]
fn _targets_used(_: &[TargetPoint]) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{AssetCategory, PublicCard, SetupSnapshot, TierThresholds};
    use crate::health::{HealthComponents, HealthScore};
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn sample_card() -> PublicCard {
        PublicCard::build_from_parts(
            SetupSnapshot {
                setup_id: Uuid::new_v4(),
                exchange: "binance".into(),
                symbol: "BTCUSDT".into(),
                timeframe: "1h".into(),
                venue_class: "crypto".into(),
                market_cap_rank: Some(1),
                direction: SetupDirection::Long,
                pattern_family: "wyckoff".into(),
                pattern_subkind: Some("spring".into()),
                ai_score: 0.82,
                entry_price: dec!(82_400),
                stop_price: dec!(81_100),
                tp1_price: Some(dec!(85_200)),
                tp2_price: Some(dec!(87_500)),
                tp3_price: None,
                current_price: Some(dec!(82_950)),
                created_at: Utc::now(),
            },
            TierThresholds::FALLBACK,
            AssetCategory::MegaCap,
        )
    }

    #[test]
    fn public_card_contains_symbol_tier_and_levels() {
        let c = sample_card();
        let n = render_public_card(&c);
        let html = n.telegram_text.as_deref().unwrap_or_default();
        assert!(html.contains("BTCUSDT"));
        assert!(html.contains("LONG"));
        assert!(html.contains("Giriş"));
        assert!(html.contains("TP1"));
        assert!(html.contains("TP2"));
        assert!(html.contains("Stop"));
        assert!(html.contains("R:R"));
        assert_eq!(n.telegram_parse_mode.as_deref(), Some("HTML"));
    }

    #[test]
    fn public_card_plain_body_has_no_tags() {
        let n = render_public_card(&sample_card());
        assert!(!n.body.contains('<'));
        assert!(!n.body.contains('>'));
    }

    fn sample_ctx(kind: LifecycleEventKind, tp_index: Option<u8>) -> LifecycleContext {
        LifecycleContext {
            setup_id: Uuid::new_v4(),
            kind,
            price: dec!(85_200),
            tp_index,
            pnl_pct: Some(3.4),
            pnl_r: Some(1.8),
            health: Some(HealthScore {
                total: 62.0,
                band: HealthBand::Warn,
                components: HealthComponents::default(),
            }),
            prev_health_band: Some(HealthBand::Healthy),
            duration_ms: Some(3_900_000),
            emitted_at: Utc::now(),
            ai_action: Some("Scale".into()),
            ai_reasoning: Some("Sağlık düşüyor — kısmi kâr al.".into()),
            ai_confidence: Some(0.72),
            exchange: "binance".into(),
            symbol: "ETHUSDT".into(),
            direction: SetupDirection::Long,
            entry_price: dec!(82_400),
            current_sl: dec!(82_500),
        }
    }

    #[test]
    fn lifecycle_tp_partial_renders_ai_block() {
        let ctx = sample_ctx(LifecycleEventKind::TpPartial, Some(1));
        let n = render_lifecycle(&ctx);
        let html = n.telegram_text.as_deref().unwrap_or_default();
        assert!(html.contains("TP1"));
        assert!(html.contains("kısmi"));
        assert!(html.contains("AI"));
        assert!(html.contains("Scale"));
        assert!(html.contains("72%"));
        assert!(html.contains("Sağlık"));
    }

    #[test]
    fn lifecycle_sl_hit_has_no_ai_block_when_absent() {
        let mut ctx = sample_ctx(LifecycleEventKind::SlHit, None);
        ctx.ai_action = None;
        ctx.ai_reasoning = None;
        ctx.ai_confidence = None;
        let n = render_lifecycle(&ctx);
        let html = n.telegram_text.as_deref().unwrap_or_default();
        assert!(html.contains("Stop"));
        assert!(!html.contains("AI:"));
    }

    #[test]
    fn header_dispatch_covers_every_kind() {
        let kinds = [
            LifecycleEventKind::EntryTouched,
            LifecycleEventKind::TpHit,
            LifecycleEventKind::TpPartial,
            LifecycleEventKind::TpFinal,
            LifecycleEventKind::SlHit,
            LifecycleEventKind::SlRatcheted,
            LifecycleEventKind::Invalidated,
            LifecycleEventKind::Cancelled,
            LifecycleEventKind::HealthWarn,
            LifecycleEventKind::HealthDanger,
        ];
        for k in kinds {
            let (emoji, headline) = header_for_kind(k, Some(1));
            assert!(!emoji.is_empty());
            assert!(!headline.is_empty());
        }
    }

    #[test]
    fn duration_formats_bucketed() {
        assert_eq!(fmt_duration_ms(30_000), "30s");
        assert_eq!(fmt_duration_ms(5 * 60_000), "5d 0s");
        assert_eq!(fmt_duration_ms(2 * 3_600_000), "2s 0d");
        assert_eq!(fmt_duration_ms(26 * 3_600_000), "1g 2s");
    }

    #[test]
    fn pct_signed_prefix() {
        assert_eq!(fmt_pct_signed(1.234), "+1.23%");
        assert_eq!(fmt_pct_signed(-2.0), "-2.00%");
    }
}
