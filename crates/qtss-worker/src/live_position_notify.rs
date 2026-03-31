//! PLAN Phase D — canlı borsa dolum özeti (`exchange_orders.venue_response`), dry paper’dan ayrı env.
//!
//! Tam pozisyon defteri (ortalama giriş, SL/TP) ayrı tablo / reconcile ile genişletilecek; burada
//! yeni **gerçekleşen** emir satırları için Telegram/webhook özeti.

use std::time::Duration;

use chrono::{DateTime, Utc};
use qtss_notify::{resolve_bilingual, Notification, NotificationChannel, NotificationDispatcher};
use qtss_storage::{
    resolve_notify_default_locale, resolve_system_csv, resolve_worker_enabled_flag,
    resolve_worker_tick_secs, ExchangeOrderRepository, ExchangeOrderRow,
};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{info, warn};

async fn live_notify_enabled(pool: &PgPool) -> bool {
    resolve_worker_enabled_flag(
        pool,
        "worker",
        "live_position_notify_enabled",
        "QTSS_NOTIFY_LIVE_POSITION_ENABLED",
        false,
    )
    .await
}

async fn live_notify_channels(pool: &PgPool) -> Vec<NotificationChannel> {
    let raw = resolve_system_csv(
        pool,
        "worker",
        "live_position_notify_channels_csv",
        "QTSS_NOTIFY_LIVE_POSITION_CHANNELS",
        "telegram",
    )
    .await;
    raw.iter()
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect()
}

fn order_summary_line_tr(row: &ExchangeOrderRow) -> String {
    order_summary_line_inner(row, false)
}

fn order_summary_line_en(row: &ExchangeOrderRow) -> String {
    order_summary_line_inner(row, true)
}

fn order_summary_line_inner(row: &ExchangeOrderRow, english: bool) -> String {
    let side = row
        .intent
        .get("side")
        .and_then(|x| x.as_str())
        .unwrap_or("?");
    let qty = row
        .intent
        .get("quantity")
        .and_then(|x| x.as_str())
        .unwrap_or("?");
    let vr = row.venue_response.as_ref();
    let st = vr
        .and_then(|v| v.get("status"))
        .and_then(|x| x.as_str())
        .unwrap_or("?");
    let exq = vr
        .and_then(|v| v.get("executedQty"))
        .and_then(|x| x.as_str())
        .unwrap_or("-");
    let quote = vr
        .and_then(|v| v.get("cummulativeQuoteQty"))
        .and_then(|x| x.as_str())
        .unwrap_or("-");
    let avg_px = match (exq.parse::<Decimal>(), quote.parse::<Decimal>()) {
        (Ok(qty_e), Ok(q)) if qty_e > Decimal::ZERO => (q / qty_e).to_string(),
        _ => "-".into(),
    };
    if english {
        format!(
            "· {} {} {} | intent {} {} | venue {} exQty {} ~px {} | user {}",
            row.exchange, row.segment, row.symbol, side, qty, st, exq, avg_px, row.user_id
        )
    } else {
        format!(
            "· {} {} {} | intent {} {} | venue {} exQty {} ~px {} | kullanıcı {}",
            row.exchange, row.segment, row.symbol, side, qty, st, exq, avg_px, row.user_id
        )
    }
}

pub async fn live_position_notify_loop(pool: PgPool) {
    if !live_notify_enabled(&pool).await {
        info!("QTSS_NOTIFY_LIVE_POSITION_ENABLED kapalı — canlı dolum bildirimi yok");
        return;
    }
    let chans = live_notify_channels(&pool).await;
    if chans.is_empty() {
        warn!("Canlı pozisyon bildirimi açık fakat QTSS_NOTIFY_LIVE_POSITION_CHANNELS boş veya tanınmadı");
        return;
    }
    let pool_tick = pool.clone();
    let pool_locale = pool.clone();
    let repo = ExchangeOrderRepository::new(pool);
    let mut cursor: Option<DateTime<Utc>> = None;

    info!("canlı dolum bildirim döngüsü (exchange_orders; poll from system_config / env)");

    loop {
        let poll_secs = resolve_worker_tick_secs(
            &pool_tick,
            "worker",
            "live_position_notify_tick_secs",
            "QTSS_NOTIFY_LIVE_TICK_SECS",
            45,
            15,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(poll_secs)).await;
        if cursor.is_none() {
            cursor = Some(Utc::now());
            continue;
        }
        let after = cursor.unwrap();
        let rows = match repo.list_filled_orders_created_after(after, 80).await {
            Ok(r) => r,
            Err(e) => {
                warn!(%e, "exchange_orders filled list");
                continue;
            }
        };
        if rows.is_empty() {
            continue;
        }
        let last_t = rows.iter().map(|r| r.created_at).max().unwrap_or(after);
        cursor = Some(last_t);

        let loc = resolve_notify_default_locale(&pool_locale).await;
        let title_tr = if rows.len() == 1 {
            let r = &rows[0];
            format!(
                "Canlı dolum — {} {} {}",
                r.exchange,
                r.symbol,
                r.intent.get("side").and_then(|x| x.as_str()).unwrap_or("?")
            )
        } else {
            format!("Canlı dolum — {} emir", rows.len())
        };
        let title_en = if rows.len() == 1 {
            let r = &rows[0];
            format!(
                "Live fill — {} {} {}",
                r.exchange,
                r.symbol,
                r.intent.get("side").and_then(|x| x.as_str()).unwrap_or("?")
            )
        } else {
            format!("Live fill — {} orders", rows.len())
        };
        let title = resolve_bilingual(&loc, &title_en, &title_tr);
        let body_tr = rows
            .iter()
            .map(order_summary_line_tr)
            .collect::<Vec<_>>()
            .join("\n");
        let body_en = rows
            .iter()
            .map(order_summary_line_en)
            .collect::<Vec<_>>()
            .join("\n");
        let body = resolve_bilingual(&loc, &body_en, &body_tr);
        let n = Notification::new(title, body);
        let d = NotificationDispatcher::from_env();
        for r in d.send_all(&chans, &n).await {
            if r.ok {
                info!(channel = ?r.channel, count = rows.len(), "live_fill bildirimi");
            } else {
                warn!(channel = ?r.channel, detail = ?r.detail, "live_fill bildirimi başarısız");
            }
        }
    }
}
