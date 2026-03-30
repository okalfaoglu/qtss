//! `notify_outbox` tüketici — `qtss-notify` ile sırayla gönderim (`docs/QTSS_CURSOR_DEV_GUIDE.md` §9.1 madde 7).

use std::time::Duration;

use qtss_notify::{Notification, NotificationChannel, NotificationDispatcher};
use qtss_storage::{resolve_worker_tick_secs, NotifyOutboxRepository};
use serde_json::json;
use sqlx::PgPool;
use tracing::{info, warn};

fn enabled() -> bool {
    std::env::var("QTSS_NOTIFY_OUTBOX_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn parse_channels(raw: &[String]) -> Vec<NotificationChannel> {
    raw.iter()
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect()
}

pub async fn notify_outbox_loop(pool: PgPool) {
    if !enabled() {
        info!("QTSS_NOTIFY_OUTBOX_ENABLED off — notify_outbox_loop exit");
        return;
    }
    let pool_tick = pool.clone();
    let repo = NotifyOutboxRepository::new(pool);
    let dispatcher = NotificationDispatcher::from_env();
    info!(
        "notify_outbox_loop: draining notify_outbox → qtss-notify (poll from system_config / env)"
    );
    loop {
        let poll_secs = resolve_worker_tick_secs(
            &pool_tick,
            "worker",
            "notify_outbox_tick_secs",
            "QTSS_NOTIFY_OUTBOX_TICK_SECS",
            10,
            2,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(poll_secs)).await;
        loop {
            let row = match repo.claim_next_pending().await {
                Ok(r) => r,
                Err(e) => {
                    warn!(%e, "notify_outbox: claim_next_pending");
                    break;
                }
            };
            let Some(row) = row else {
                break;
            };
            let channels = parse_channels(&row.channels);
            if channels.is_empty() {
                let msg = "no valid channels in notify_outbox.channels JSON";
                if let Err(e) = repo.mark_failed(row.id, msg).await {
                    warn!(%e, "notify_outbox: mark_failed");
                }
                continue;
            }
            let n = Notification::new(row.title.clone(), row.body.clone());
            let receipts = dispatcher.send_all(&channels, &n).await;
            let detail = serde_json::to_value(&receipts).unwrap_or_else(|_| json!([]));
            let all_ok = receipts.iter().all(|r| r.ok);
            if all_ok {
                if let Err(e) = repo.mark_sent(row.id, detail).await {
                    warn!(%e, id = %row.id, "notify_outbox: mark_sent");
                } else {
                    info!(id = %row.id, "notify_outbox: sent");
                }
            } else {
                let err = receipts
                    .iter()
                    .find(|r| !r.ok)
                    .and_then(|r| r.detail.clone())
                    .unwrap_or_else(|| "delivery failed".into());
                if let Err(e) = repo.mark_failed(row.id, &err).await {
                    warn!(%e, id = %row.id, "notify_outbox: mark_failed");
                } else {
                    warn!(id = %row.id, %err, "notify_outbox: failed");
                }
            }
        }
    }
}
