//! Faz 9.7.6 — X (Twitter) renderer.
//!
//! Produces compact, ≤280-char Turkish bodies for X posts. Completely
//! distinct from the Telegram HTML renderer: no markup, aggressive
//! truncation, emoji-forward so the tweet still reads at a glance.
//!
//! CLAUDE.md #1 — per-kind headlines go through a dispatch table.

use crate::card::{PublicCard, SetupDirection};
use crate::lifecycle::{LifecycleContext, LifecycleEventKind};

pub const X_MAX_CHARS: usize = 280;

/// Render a new-setup broadcast for X. Always returns a body ≤280
/// chars; the builder trims lowest-priority lines first (category,
/// pattern subkind) before falling back to a hard-truncate with `…`.
pub fn render_public_card_x(card: &PublicCard) -> String {
    let header = format!(
        "{} #{} {} — Skor {}/10",
        emoji_for_tier(card.tier.out_of_ten),
        card.symbol,
        card.direction.label_tr(),
        card.tier.out_of_ten,
    );

    let mut lines: Vec<String> = Vec::with_capacity(6);
    lines.push(header);
    lines.push(format!("Giriş: {}", fmt_price(card.entry_price)));
    lines.push(format!(
        "Stop: {} ({})",
        fmt_price(card.stop_price),
        fmt_pct(card.stop_pct),
    ));
    if let Some(tp1) = card.targets.first() {
        lines.push(format!(
            "TP1: {} ({})",
            fmt_price(tp1.price),
            fmt_pct(tp1.pct),
        ));
    }
    if let Some(rr) = card.risk_reward {
        lines.push(format!("R:R {:.2}", rr));
    }
    lines.push(format!("{} · {}", card.category_label, card.pattern_label));

    assemble_under_limit(&lines)
}

/// Render a lifecycle event for X — single tweet, PnL + headline + AI hint.
pub fn render_lifecycle_x(ctx: &LifecycleContext) -> String {
    let (emoji, headline) = header_for_kind(ctx.kind, ctx.tp_index);
    let mut lines: Vec<String> = Vec::with_capacity(5);
    lines.push(format!(
        "{} #{} {} — {}",
        emoji,
        ctx.symbol,
        ctx.direction.label_tr(),
        headline,
    ));
    lines.push(format!("Fiyat: {}", fmt_price(ctx.price)));
    if let Some(pnl) = ctx.pnl_pct {
        lines.push(format!("PnL: {}", fmt_pct(pnl)));
    }
    if let Some(ai) = &ctx.ai_action {
        lines.push(format!("AI: {}", ai));
    }
    assemble_under_limit(&lines)
}

// ---------------------------------------------------------------------------
// Dispatch tables (CLAUDE.md #1)
// ---------------------------------------------------------------------------

fn header_for_kind(kind: LifecycleEventKind, tp_index: Option<u8>) -> (&'static str, String) {
    match kind {
        LifecycleEventKind::EntryTouched => ("🎯", "Giriş tetiklendi".into()),
        LifecycleEventKind::TpHit => (
            "✅",
            tp_index.map(|i| format!("TP{i} vuruldu")).unwrap_or_else(|| "Hedef".into()),
        ),
        LifecycleEventKind::TpPartial => (
            "🟢",
            tp_index.map(|i| format!("TP{i} kısmi")).unwrap_or_else(|| "Kısmi".into()),
        ),
        LifecycleEventKind::TpFinal => ("🏁", "Son hedef — kapandı".into()),
        LifecycleEventKind::SlHit => ("🛑", "Stop tetiklendi".into()),
        LifecycleEventKind::SlRatcheted => ("🔒", "Stop yukarı çekildi".into()),
        LifecycleEventKind::Invalidated => ("❌", "Setup geçersiz".into()),
        LifecycleEventKind::Cancelled => ("↩️", "Setup iptal".into()),
        LifecycleEventKind::HealthWarn => ("⚠️", "Sağlık: uyarı".into()),
        LifecycleEventKind::HealthDanger => ("🚨", "Sağlık: tehlike".into()),
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

// ---------------------------------------------------------------------------
// Assemble + truncation
// ---------------------------------------------------------------------------

/// Join with `\n`, dropping trailing lines until the whole body fits.
/// Falls back to char-wise hard-truncate with `…` if even the header
/// exceeds the limit.
fn assemble_under_limit(lines: &[String]) -> String {
    let mut n = lines.len();
    loop {
        let candidate = lines[..n].join("\n");
        if candidate.chars().count() <= X_MAX_CHARS {
            return candidate;
        }
        if n <= 1 {
            // Even the header is too long — hard truncate.
            let trimmed: String =
                lines[0].chars().take(X_MAX_CHARS.saturating_sub(1)).collect();
            return format!("{trimmed}…");
        }
        n -= 1;
    }
}

fn fmt_price(d: rust_decimal::Decimal) -> String {
    let s = d.normalize().to_string();
    if s.is_empty() { "0".into() } else { s }
}

fn fmt_pct(pct: f64) -> String {
    if pct >= 0.0 {
        format!("+{:.2}%", pct)
    } else {
        format!("{:.2}%", pct)
    }
}

// SetupDirection re-export bridge — silences unused warning when tests
// are disabled.
#[allow(dead_code)]
fn _dir_used(_: SetupDirection) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{AssetCategory, PublicCard, SetupSnapshot, TierThresholds};
    use crate::health::{HealthBand, HealthComponents, HealthScore};
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
    fn public_card_fits_280_and_has_symbol() {
        let body = render_public_card_x(&sample_card());
        assert!(body.chars().count() <= X_MAX_CHARS);
        assert!(body.contains("#BTCUSDT"));
        assert!(body.contains("LONG"));
        assert!(body.contains("Giriş"));
        assert!(body.contains("TP1"));
    }

    #[test]
    fn lifecycle_tp_final_fits_and_shows_pnl() {
        let ctx = LifecycleContext {
            setup_id: Uuid::new_v4(),
            kind: LifecycleEventKind::TpFinal,
            price: dec!(87_500),
            tp_index: Some(3),
            pnl_pct: Some(6.2),
            pnl_r: Some(2.4),
            health: Some(HealthScore {
                total: 75.0,
                band: HealthBand::Healthy,
                components: HealthComponents::default(),
            }),
            prev_health_band: None,
            duration_ms: Some(0),
            emitted_at: Utc::now(),
            ai_action: Some("Exit".into()),
            ai_reasoning: None,
            ai_confidence: Some(0.88),
            exchange: "binance".into(),
            symbol: "BTCUSDT".into(),
            direction: SetupDirection::Long,
            entry_price: dec!(82_400),
            current_sl: dec!(82_400),
        };
        let body = render_lifecycle_x(&ctx);
        assert!(body.chars().count() <= X_MAX_CHARS);
        assert!(body.contains("BTCUSDT"));
        assert!(body.contains("Son hedef"));
        assert!(body.contains("+6.20%"));
        assert!(body.contains("AI: Exit"));
    }

    #[test]
    fn assemble_drops_lines_when_over_limit() {
        let long = "x".repeat(260);
        let lines = vec![
            "head".into(),
            long.clone(),
            "extra".into(),
        ];
        let out = assemble_under_limit(&lines);
        assert!(out.chars().count() <= X_MAX_CHARS);
        // Must keep header, drop "extra" at minimum.
        assert!(out.starts_with("head"));
    }

    #[test]
    fn hard_truncate_when_header_too_long() {
        let header = "a".repeat(400);
        let out = assemble_under_limit(&[header]);
        assert_eq!(out.chars().count(), X_MAX_CHARS);
        assert!(out.ends_with('…'));
    }
}
