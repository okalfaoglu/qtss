//! Günlük realized P&L / drawdown eşiği — [`qtss_common::halt_trading`] (dev guide ADIM 10, §3.6).
//! Açık pozisyonları otomatik kapatmaz; stratejiler yeni emir vermez. Acil kapatma için `position_manager` / manuel.
//!
//! Öncelik: `QTSS_MAX_DRAWDOWN_PCT` + `QTSS_KILL_SWITCH_REFERENCE_EQUITY_USDT` tanımlıysa eşik
//! `-(equity * pct / 100)` olur. Aksi halde `QTSS_KILL_SWITCH_DAILY_LOSS_USDT` (varsayılan: çok büyük).

use std::ops::Neg;
use std::str::FromStr;
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

fn reference_equity_usdt() -> Decimal {
    std::env::var("QTSS_KILL_SWITCH_REFERENCE_EQUITY_USDT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or_else(|| Decimal::new(100_000, 0))
}

/// `QTSS_MAX_DRAWDOWN_PCT` (örn. 5.0 = %5) ile günlük kayıp limiti (USDT, negatif eşik).
fn trigger_from_drawdown_pct() -> Option<Decimal> {
    let raw = std::env::var("QTSS_MAX_DRAWDOWN_PCT").ok()?;
    let pct = Decimal::from_str(raw.trim()).ok()?;
    if pct <= Decimal::ZERO {
        return None;
    }
    let eq = reference_equity_usdt();
    let loss = eq * pct / Decimal::from(100u32);
    Some(loss.neg())
}

fn trigger_from_daily_loss_env() -> Decimal {
    std::env::var("QTSS_KILL_SWITCH_DAILY_LOSS_USDT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or(Decimal::new(1_000_000, 0))
        .neg()
}

fn effective_trigger_neg() -> Decimal {
    trigger_from_drawdown_pct().unwrap_or_else(trigger_from_daily_loss_env)
}

pub async fn kill_switch_loop(pool: PgPool) {
    if !enabled() {
        info!("QTSS_KILL_SWITCH_ENABLED kapalı — kill_switch_loop çıkıyor");
        return;
    }
    let tick = Duration::from_secs(tick_secs());
    let trigger_neg = effective_trigger_neg();
    info!(
        poll_secs = tick.as_secs(),
        %trigger_neg,
        "kill_switch_loop: günlük realized P&L < eşik ise halt (QTSS_MAX_DRAWDOWN_PCT veya QTSS_KILL_SWITCH_DAILY_LOSS_USDT)"
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
            warn!(%sum, %trigger_neg, "kill_switch: eşik altı — halt (yeni emirleri stratejiler engellemeli)");
            halt_trading();
        }
    }
}
