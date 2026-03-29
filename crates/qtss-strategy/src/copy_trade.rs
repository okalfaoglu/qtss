//! Copy-trade izleme — aktif abonelikleri DB’den okur (dev guide ADIM 7, §3.4).

use std::sync::Arc;
use std::time::Duration;

use qtss_common::is_trading_halted;
use qtss_domain::CopyRule;
use qtss_storage::CopySubscriptionRepository;
use sqlx::PgPool;
use tracing::{info, warn};

fn tick_secs() -> u64 {
    std::env::var("QTSS_COPY_TRADE_STRATEGY_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120)
        .max(30)
}

pub async fn run(pool: PgPool, _gateway: Arc<dyn qtss_execution::ExecutionGateway>) {
    let tick = Duration::from_secs(tick_secs());
    let repo = CopySubscriptionRepository::new(pool);
    info!(poll_secs = tick.as_secs(), "copy_trade strateji döngüsü");
    loop {
        tokio::time::sleep(tick).await;
        if is_trading_halted() {
            continue;
        }
        match repo.list_active_subscriptions().await {
            Ok(rows) => {
                for r in rows {
                    let rule: Result<CopyRule, _> = serde_json::from_value(r.rule.clone());
                    match rule {
                        Ok(rule) => {
                            info!(
                                sub_id = %r.id,
                                leader = %r.leader_user_id,
                                follower = %r.follower_user_id,
                                mult = %rule.size_multiplier,
                                "copy_trade: aktif kural (Nansen/perp ile eşleştirme sonraki adım)"
                            );
                        }
                        Err(e) => warn!(sub_id = %r.id, %e, "copy_trade: CopyRule parse"),
                    }
                }
            }
            Err(e) => warn!(%e, "copy_trade list_active_subscriptions"),
        }
    }
}
