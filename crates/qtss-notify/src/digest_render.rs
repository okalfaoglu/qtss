//! Faz 9.7.7 — Daily digest renderer.
//!
//! Pure function over [`DigestAggregate`] + user context → a Turkish
//! HTML [`Notification`] suitable for Telegram. Non-Telegram channels
//! get a plain-text body from the same source.

use chrono::{DateTime, Duration, FixedOffset, Utc};
use qtss_storage::DigestAggregate;

use crate::telegram_html::escape_telegram_html;
use crate::types::Notification;

pub struct DigestRenderInput<'a> {
    pub agg: &'a DigestAggregate,
    pub tz_offset_minutes: i32,
    /// User-visible local date (the "for day" label, e.g. "2026-04-18").
    pub local_label_at: DateTime<Utc>,
}

pub fn render_digest(input: &DigestRenderInput<'_>) -> Notification {
    let offset = FixedOffset::east_opt(input.tz_offset_minutes * 60)
        .unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
    let local = input.local_label_at.with_timezone(&offset);
    let date_label = local.format("%Y-%m-%d").to_string();
    let tz_label = fmt_offset(input.tz_offset_minutes);

    let win_h = (input.agg.window_end_utc - input.agg.window_start_utc).num_hours().max(1);
    let pnl_emoji = if input.agg.total_pnl_pct >= 0.0 { "📈" } else { "📉" };
    let pnl_str = fmt_pct_signed(input.agg.total_pnl_pct);

    let title = format!(
        "📊 Günlük Özet — {} ({})",
        date_label, tz_label,
    );

    let mut html = String::with_capacity(512);
    html.push_str(&format!(
        "<b>📊 Günlük Özet</b> — <i>{} ({})</i>\n",
        escape_telegram_html(&date_label),
        escape_telegram_html(&tz_label),
    ));
    html.push_str(&format!("<i>Son {win_h} saat</i>\n\n"));

    html.push_str(&format!("🆕 Açılan setup: <b>{}</b>\n", input.agg.opened));
    html.push_str(&format!("🏁 Kapanan: <b>{}</b>\n", input.agg.closed));
    html.push_str(&format!(
        "  ✅ TP final: <b>{}</b>  🛑 SL: <b>{}</b>  ❌ Geçersiz: <b>{}</b>  ↩️ İptal: <b>{}</b>\n",
        input.agg.tp_final, input.agg.sl_hit, input.agg.invalidated, input.agg.cancelled,
    ));
    html.push_str(&format!(
        "\n{} Toplam PnL: <b>{}</b>\n",
        pnl_emoji, pnl_str,
    ));
    if let Some(h) = input.agg.avg_open_health {
        html.push_str(&format!("🫀 Açık setup sağlık ort.: <b>{:.0}</b>\n", h));
    }

    let body_plain = html_to_plain(&html);
    Notification::new(title, body_plain).with_telegram_html_message(html)
}

fn fmt_offset(minutes: i32) -> String {
    let sign = if minutes >= 0 { '+' } else { '-' };
    let m = minutes.abs();
    format!("UTC{sign}{:02}:{:02}", m / 60, m % 60)
}

fn fmt_pct_signed(pct: f64) -> String {
    if pct >= 0.0 {
        format!("+{:.2}%", pct)
    } else {
        format!("{:.2}%", pct)
    }
}

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

/// Convenience: UTC window for "the last `hours` hours ending at `now`".
pub fn default_window(now_utc: DateTime<Utc>, hours: i64) -> (DateTime<Utc>, DateTime<Utc>) {
    let from = now_utc - Duration::hours(hours.max(1));
    (from, now_utc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agg() -> DigestAggregate {
        let end = Utc::now();
        let start = end - Duration::hours(24);
        DigestAggregate {
            window_start_utc: start,
            window_end_utc: end,
            opened: 7,
            closed: 4,
            tp_final: 2,
            sl_hit: 1,
            invalidated: 1,
            cancelled: 0,
            total_pnl_pct: 3.25,
            avg_open_health: Some(62.4),
        }
    }

    #[test]
    fn digest_html_contains_all_counts() {
        let input = DigestRenderInput {
            agg: &agg(),
            tz_offset_minutes: 180,
            local_label_at: Utc::now(),
        };
        let n = render_digest(&input);
        let html = n.telegram_text.as_deref().unwrap_or_default();
        assert!(html.contains("Günlük Özet"));
        assert!(html.contains("UTC+03:00"));
        assert!(html.contains("Açılan setup"));
        assert!(html.contains("<b>7</b>"));
        assert!(html.contains("Toplam PnL"));
        assert!(html.contains("+3.25%"));
        assert!(html.contains("62"));
    }

    #[test]
    fn digest_plain_has_no_tags() {
        let input = DigestRenderInput {
            agg: &agg(),
            tz_offset_minutes: -300,
            local_label_at: Utc::now(),
        };
        let n = render_digest(&input);
        assert!(!n.body.contains('<'));
        assert!(n.body.contains("UTC-05:00"));
    }

    #[test]
    fn negative_pnl_renders_downward_emoji() {
        let mut a = agg();
        a.total_pnl_pct = -1.2;
        let input = DigestRenderInput {
            agg: &a,
            tz_offset_minutes: 0,
            local_label_at: Utc::now(),
        };
        let n = render_digest(&input);
        let html = n.telegram_text.as_deref().unwrap_or_default();
        assert!(html.contains("📉"));
        assert!(html.contains("-1.20%"));
    }

    #[test]
    fn default_window_spans_requested_hours() {
        let now = Utc::now();
        let (from, to) = default_window(now, 24);
        assert_eq!((to - from).num_hours(), 24);
        assert_eq!(to, now);
    }
}
