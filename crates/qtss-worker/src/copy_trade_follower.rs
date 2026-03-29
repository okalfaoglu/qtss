//! Aktif copy aboneliklerini periyodik izler (dev guide §3.4 — yürütme iskeleti).
//!
//! Tam çoğaltma: lider dolum akışı + `CopyRule` + `ExecutionGateway` bağlanacak.

use std::time::Duration;

use qtss_storage::CopySubscriptionRepository;
use sqlx::PgPool;
use tracing::{info, warn};

fn enabled() -> bool {
    std::env::var("QTSS_COPY_TRADE_FOLLOWER_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn tick_secs() -> u64 {
    std::env::var("QTSS_COPY_TRADE_FOLLOWER_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300)
        .max(30)
}

pub async fn copy_trade_follower_loop(pool: PgPool) {
    if !enabled() {
        info!("QTSS_COPY_TRADE_FOLLOWER_ENABLED kapalı — copy_trade_follower_loop çıkıyor");
        return;
    }
    let tick = Duration::from_secs(tick_secs());
    let repo = CopySubscriptionRepository::new(pool);
    info!(poll_secs = tick.as_secs(), "copy_trade_follower_loop (abonelik izleme)");
    loop {
        tokio::time::sleep(tick).await;
        match repo.list_active_subscriptions().await {
            Ok(rows) => {
                if rows.is_empty() {
                    tracing::debug!("copy_trade_follower: aktif abonelik yok");
                } else {
                    info!(count = rows.len(), "copy_trade_follower: aktif abonelik");
                }
            }
            Err(e) => warn!(%e, "copy_trade_follower: list_active_subscriptions"),
        }
    }
}
