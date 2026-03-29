//! Açık dolumları izler; SL/TP için altyapı (dev guide ADIM 9, §3.5).
//!
//! Şu an: son dolumları sayar ve yapılandırma notu loglar. Gerçek koruyucu emirler için
//! pozisyon defteri + giriş fiyatı kaynağı genişletildiğinde `ExecutionGateway` ile bağlanır.

use std::time::Duration;

use chrono::Utc;
use qtss_storage::ExchangeOrderRepository;
use sqlx::PgPool;
use tracing::{info, warn};

fn enabled() -> bool {
    std::env::var("QTSS_POSITION_MANAGER_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn tick_secs() -> u64 {
    std::env::var("QTSS_POSITION_MANAGER_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10)
        .max(5)
}

pub async fn position_manager_loop(pool: PgPool) {
    if !enabled() {
        info!("QTSS_POSITION_MANAGER_ENABLED kapalı — position_manager_loop çıkıyor");
        return;
    }
    let tick = Duration::from_secs(tick_secs());
    let repo = ExchangeOrderRepository::new(pool.clone());
    info!(poll_secs = tick.as_secs(), "position_manager_loop (izleme modu)");
    loop {
        tokio::time::sleep(tick).await;
        let since = Utc::now() - chrono::Duration::hours(24 * 7);
        match repo.list_filled_orders_created_after(since, 500).await {
            Ok(rows) => {
                if !rows.is_empty() {
                    tracing::debug!(
                        filled_recent = rows.len(),
                        "position_manager: yakın dolumlar (SL/TP motoru için genişletilebilir)"
                    );
                }
            }
            Err(e) => warn!(%e, "position_manager: list_filled_orders_created_after"),
        }
    }
}
