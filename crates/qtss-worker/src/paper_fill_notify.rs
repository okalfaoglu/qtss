//! Dry (paper) dolumları için isteğe bağlı bildirim — SPEC F7 / PLAN Phase D (pozisyon MVP).
//! Gerçek borsa pozisyonu: ileride `exchange_orders` / reconcile ile genişletilebilir.

use std::time::Duration;

use chrono::{DateTime, Utc};
use qtss_notify::{resolve_bilingual, Notification, NotificationChannel, NotificationDispatcher};
use qtss_storage::{
    resolve_notify_default_locale, resolve_system_csv, resolve_worker_enabled_flag,
    resolve_worker_tick_secs, PaperFillRow, PaperLedgerRepository,
};
use sqlx::PgPool;
use tracing::{info, warn};

async fn notify_paper_position_enabled(pool: &PgPool) -> bool {
    resolve_worker_enabled_flag(
        pool,
        "worker",
        "paper_position_notify_enabled",
        "QTSS_NOTIFY_PAPER_POSITION_ENABLED",
        false,
    )
    .await
        || resolve_worker_enabled_flag(
            pool,
            "worker",
            "paper_position_notify_enabled",
            "QTSS_NOTIFY_POSITION_ENABLED",
            false,
        )
        .await
}

async fn position_notify_channels(pool: &PgPool) -> Vec<NotificationChannel> {
    let raw = resolve_system_csv(
        pool,
        "worker",
        "paper_position_notify_channels_csv",
        "QTSS_NOTIFY_PAPER_POSITION_CHANNELS",
        "telegram",
    )
    .await;
    if raw.is_empty() {
        return vec![];
    }
    raw.iter()
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect()
}

fn fill_line_tr(f: &PaperFillRow) -> String {
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

fn fill_line_en(f: &PaperFillRow) -> String {
    format!(
        "· {} {} {} | {} qty {} @ {} (fee {}) | quote {}",
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
    let enabled = notify_paper_position_enabled(&pool).await;
    if !enabled {
        info!("QTSS_NOTIFY_PAPER_POSITION_ENABLED kapalı — paper dolum bildirimi yok");
        return;
    }
    let chans = position_notify_channels(&pool).await;
    if chans.is_empty() {
        warn!("Paper pozisyon bildirimi açık fakat kanal listesi boş (QTSS_NOTIFY_PAPER_POSITION_CHANNELS / QTSS_NOTIFY_POSITION_CHANNELS)");
        return;
    }
    let pool_tick = pool.clone();
    let pool_locale = pool.clone();
    let repo = PaperLedgerRepository::new(pool);
    let mut cursor: Option<DateTime<Utc>> = None;

    info!("paper dolum bildirim döngüsü (poll from system_config / env)");

    loop {
        let poll_secs = resolve_worker_tick_secs(
            &pool_tick,
            "worker",
            "paper_position_notify_tick_secs",
            "QTSS_NOTIFY_POSITION_TICK_SECS",
            30,
            10,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(poll_secs)).await;
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
        let last_t = fills.iter().map(|f| f.created_at).max().unwrap_or(after);
        cursor = Some(last_t);

        let loc = resolve_notify_default_locale(&pool_locale).await;
        let title_tr = if fills.len() == 1 {
            let f = &fills[0];
            format!(
                "Dry dolum — {} {} {}",
                f.exchange,
                f.symbol,
                f.side.to_uppercase()
            )
        } else {
            format!("Dry dolum — {} işlem", fills.len())
        };
        let title_en = if fills.len() == 1 {
            let f = &fills[0];
            format!(
                "Dry fill — {} {} {}",
                f.exchange,
                f.symbol,
                f.side.to_uppercase()
            )
        } else {
            format!("Dry fill — {} trades", fills.len())
        };
        let title = resolve_bilingual(&loc, &title_en, &title_tr);
        let body_tr = fills
            .iter()
            .map(fill_line_tr)
            .collect::<Vec<_>>()
            .join("\n");
        let body_en = fills
            .iter()
            .map(fill_line_en)
            .collect::<Vec<_>>()
            .join("\n");
        let body = resolve_bilingual(&loc, &body_en, &body_tr);
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
