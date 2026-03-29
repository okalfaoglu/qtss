//! Dry (paper) dolumları için isteğe bağlı bildirim — SPEC F7 / PLAN Phase D (pozisyon MVP).
//! Gerçek borsa pozisyonu: ileride `exchange_orders` / reconcile ile genişletilebilir.

use std::time::Duration;

use chrono::{DateTime, Utc};
use qtss_notify::{Notification, NotificationChannel, NotificationDispatcher};
use qtss_storage::{PaperFillRow, PaperLedgerRepository};
use sqlx::PgPool;
use tracing::{info, warn};

fn notify_paper_position_enabled() -> bool {
    std::env::var("QTSS_NOTIFY_PAPER_POSITION_ENABLED")
        .or_else(|_| std::env::var("QTSS_NOTIFY_POSITION_ENABLED"))
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn position_notify_channels_from_env() -> Vec<NotificationChannel> {
    let raw = std::env::var("QTSS_NOTIFY_PAPER_POSITION_CHANNELS")
        .or_else(|_| std::env::var("QTSS_NOTIFY_POSITION_CHANNELS"))
        .unwrap_or_else(|_| "telegram".into());
    raw.split(',')
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect()
}

fn position_notify_tick_secs() -> u64 {
    std::env::var("QTSS_NOTIFY_POSITION_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30)
        .max(10)
}

fn fill_line(f: &PaperFillRow) -> String {
    format!(
        "· {} {} {} | {} qty {} @ {} (ücret {}) | quote {}",
        f.exchange,
        f.segment,
        f.symbol,
        f.side.to_uppercase(),
        f.quantity,
        f.avg_price,
        f.fee,
        f.quote_balance_after
    )
}

pub async fn paper_position_notify_loop(pool: PgPool) {
    if !notify_paper_position_enabled() {
        info!("QTSS_NOTIFY_PAPER_POSITION_ENABLED kapalı — paper dolum bildirimi yok");
        return;
    }
    let chans = position_notify_channels_from_env();
    if chans.is_empty() {
        warn!("Paper pozisyon bildirimi açık fakat kanal listesi boş (QTSS_NOTIFY_PAPER_POSITION_CHANNELS / QTSS_NOTIFY_POSITION_CHANNELS)");
        return;
    }
    let tick = Duration::from_secs(position_notify_tick_secs());
    let repo = PaperLedgerRepository::new(pool);
    let mut cursor: Option<DateTime<Utc>> = None;

    info!(poll_secs = tick.as_secs(), "paper dolum bildirim döngüsü");

    loop {
        tokio::time::sleep(tick).await;
        if cursor.is_none() {
            cursor = Some(Utc::now());
            continue;
        }
        let after = cursor.unwrap();
        let fills = match repo.list_fills_created_after(after, 50).await {
            Ok(f) => f,
            Err(e) => {
                warn!(%e, "paper_fills listesi");
                continue;
            }
        };
        if fills.is_empty() {
            continue;
        }
        let last_t = fills
            .iter()
            .map(|f| f.created_at)
            .max()
            .unwrap_or(after);
        cursor = Some(last_t);

        let title = if fills.len() == 1 {
            let f = &fills[0];
            format!(
                "Dry dolum — {} {} {}",
                f.exchange, f.symbol, f.side.to_uppercase()
            )
        } else {
            format!("Dry dolum — {} işlem", fills.len())
        };
        let body = fills.iter().map(|f| fill_line(f)).collect::<Vec<_>>().join("\n");
        let n = Notification::new(title, body);
        let d = NotificationDispatcher::from_env();
        for r in d.send_all(&chans, &n).await {
            if r.ok {
                info!(channel = ?r.channel, count = fills.len(), "paper_fill bildirimi");
            } else {
                warn!(channel = ?r.channel, detail = ?r.detail, "paper_fill bildirimi başarısız");
            }
        }
    }
}
