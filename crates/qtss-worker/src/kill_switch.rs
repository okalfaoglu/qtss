//! Günlük realized P&L eşiği — [`qtss_common::halt_trading`] (dev guide ADIM 10, §3.6).
//!
//! `pnl_rollups` günlük realized toplamı `QTSS_KILL_SWITCH_DAILY_LOSS_USDT` altına düşerse durdurur.
//! Rollup’lar çoğu kurulumda henüz anlamlı realized üretmeyebilir — eşiği ihtiyaca göre ayarlayın.

use std::ops::Neg;
use std::time::Duration;

use qtss_common::{halt_trading, is_trading_halted};
use qtss_storage::sum_today_daily_realized_pnl;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{info, warn};

fn enabled() -> bool {
    std::env::var("QTSS_KILL_SWITCH_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn tick_secs() -> u64 {
    std::env::var("QTSS_KILL_SWITCH_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60)
        .max(15)
}

fn loss_trigger_usdt() -> Decimal {
    std::env::var("QTSS_KILL_SWITCH_DAILY_LOSS_USDT")
        .ok()
        .and_then(|s| s.parse::<Decimal>().ok())
        .unwrap_or(Decimal::new(1_000_000, 0))
}

pub async fn kill_switch_loop(pool: PgPool) {
    if !enabled() {
        info!("QTSS_KILL_SWITCH_ENABLED kapalı — kill_switch_loop çıkıyor");
        return;
    }
    let tick = Duration::from_secs(tick_secs());
    let trigger_neg = loss_trigger_usdt().neg();
    info!(
        poll_secs = tick.as_secs(),
        %trigger_neg,
        "kill_switch_loop: günlük realized < bu değer ise halt"
    );
    loop {
        tokio::time::sleep(tick).await;
        if is_trading_halted() {
            continue;
        }
        let sum = match sum_today_daily_realized_pnl(&pool).await {
            Ok(s) => s,
            Err(e) => {
                warn!(%e, "kill_switch: sum_today_daily_realized_pnl");
                continue;
            }
        };
        if sum < trigger_neg {
            warn!(%sum, %trigger_neg, "kill_switch: günlük realized eşik altı — halt");
            halt_trading();
        }
    }
}
