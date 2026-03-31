//! Günlük realized P&L / drawdown eşiği — [`qtss_common::halt_trading`] (dev guide ADIM 10, §3.6).
//! Açık pozisyonları otomatik kapatmaz; stratejiler yeni emir vermez. Acil kapatma için `position_manager` / manuel.
//!
//! Öncelik: `QTSS_MAX_DRAWDOWN_PCT` + `QTSS_KILL_SWITCH_REFERENCE_EQUITY_USDT` tanımlıysa eşik
//! `-(equity * pct / 100)` olur. Aksi halde `QTSS_KILL_SWITCH_DAILY_LOSS_USDT` (varsayılan: çok büyük).
//!
//! **API süreçleri ayrı:** `kill_switch_trading_halted` `app_config` üzerinden tutulur; `kill_switch_db_sync_loop`
//! worker sürecinde atomik bayrağı DB ile hizalar (`POST /api/v1/admin/kill-switch/reset`).

use std::ops::Neg;
use std::str::FromStr;
use std::time::Duration;

use qtss_common::{halt_trading, is_trading_halted};
use qtss_storage::{
    resolve_worker_enabled_flag, resolve_worker_tick_secs, sum_today_daily_realized_pnl,
    AppConfigRepository, SystemConfigRepository,
};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{info, warn};

pub const KILL_SWITCH_APP_CONFIG_KEY: &str = "kill_switch_trading_halted";

fn enabled() -> bool {
    // Legacy env-only gate (bootstrapping / emergency).
    std::env::var("QTSS_KILL_SWITCH_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn decimal_from_json_value(v: &serde_json::Value) -> Option<Decimal> {
    v.get("value")
        .and_then(|x| x.as_str())
        .or_else(|| v.as_str())
        .and_then(|s| Decimal::from_str(s.trim()).ok())
}

async fn reference_equity_usdt(pool: &PgPool) -> Decimal {
    let repo = SystemConfigRepository::new(pool.clone());
    if let Ok(Some(row)) = repo.get("worker", "kill_switch_reference_equity_usdt").await {
        if let Some(d) = decimal_from_json_value(&row.value) {
            return d;
        }
    }
    std::env::var("QTSS_KILL_SWITCH_REFERENCE_EQUITY_USDT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or_else(|| Decimal::new(100_000, 0))
}

/// `QTSS_MAX_DRAWDOWN_PCT` (örn. 5.0 = %5) ile günlük kayıp limiti (USDT, negatif eşik).
async fn max_drawdown_pct(pool: &PgPool) -> Option<Decimal> {
    let repo = SystemConfigRepository::new(pool.clone());
    if let Ok(Some(row)) = repo.get("worker", "max_drawdown_pct").await {
        if let Some(d) = decimal_from_json_value(&row.value) {
            return Some(d);
        }
    }
    std::env::var("QTSS_MAX_DRAWDOWN_PCT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
}

async fn trigger_from_drawdown_pct(pool: &PgPool) -> Option<Decimal> {
    let pct = max_drawdown_pct(pool).await?;
    if pct <= Decimal::ZERO {
        return None;
    }
    let eq = reference_equity_usdt(pool).await;
    let loss = eq * pct / Decimal::from(100u32);
    Some(loss.neg())
}

async fn trigger_from_daily_loss(pool: &PgPool) -> Decimal {
    let repo = SystemConfigRepository::new(pool.clone());
    if let Ok(Some(row)) = repo.get("worker", "kill_switch_daily_loss_usdt").await {
        if let Some(d) = decimal_from_json_value(&row.value) {
            return d.neg();
        }
    }
    std::env::var("QTSS_KILL_SWITCH_DAILY_LOSS_USDT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or(Decimal::new(1_000_000, 0))
        .neg()
}

async fn effective_trigger_neg(pool: &PgPool) -> Decimal {
    if let Some(d) = trigger_from_drawdown_pct(pool).await {
        d
    } else {
        trigger_from_daily_loss(pool).await
    }
}

async fn persist_halt_flag(pool: &PgPool, halted: bool) {
    let repo = AppConfigRepository::new(pool.clone());
    if let Err(e) = repo
        .upsert(
            KILL_SWITCH_APP_CONFIG_KEY,
            serde_json::json!(halted),
            Some("Trading halt flag — API/worker senkronu (kill_switch_db_sync_loop)"),
            None,
        )
        .await
    {
        warn!(%e, "kill_switch: app_config kill_switch_trading_halted yazılamadı");
    }
}

/// Worker başlangıcında DB’de halt=true ise süreç bayrağını yükle (restart sonrası).
pub async fn apply_initial_halt_from_db(pool: &PgPool) {
    match AppConfigRepository::get_value_json(pool, KILL_SWITCH_APP_CONFIG_KEY).await {
        Ok(Some(v)) if v.as_bool() == Some(true) => {
            halt_trading();
            info!("kill_switch: app_config kill_switch_trading_halted=true — başlangıç halt");
        }
        Ok(_) => {}
        Err(e) => warn!(%e, "kill_switch: başlangıç app_config okunamadı"),
    }
}

/// `app_config` ↔ `qtss_common` atomik halt senkronu (API reset ve çok süreç).
pub async fn kill_switch_db_sync_loop(pool: PgPool) {
    let pool_tick = pool.clone();
    info!("kill_switch_db_sync_loop: app_config kill_switch_trading_halted senkronu (poll from system_config / env)");
    loop {
        let poll_secs = resolve_worker_tick_secs(
            &pool_tick,
            "worker",
            "kill_switch_db_sync_tick_secs",
            "QTSS_KILL_SWITCH_DB_SYNC_SECS",
            5,
            2,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(poll_secs)).await;
        match AppConfigRepository::get_value_json(&pool, KILL_SWITCH_APP_CONFIG_KEY).await {
            Ok(Some(v)) => match v.as_bool() {
                Some(false) => {
                    if is_trading_halted() {
                        qtss_common::clear_trading_halt();
                        info!("kill_switch_db_sync: halt kaldırıldı (app_config=false)");
                    }
                }
                Some(true) => {
                    if !is_trading_halted() {
                        halt_trading();
                        info!("kill_switch_db_sync: halt uygulandı (app_config=true)");
                    }
                }
                None => {}
            },
            Ok(None) => {}
            Err(e) => warn!(%e, "kill_switch_db_sync: app_config okunamadı"),
        }
    }
}

pub async fn kill_switch_loop(pool: PgPool) {
    let enabled_db = resolve_worker_enabled_flag(
        &pool,
        "worker",
        "kill_switch_enabled",
        "QTSS_KILL_SWITCH_ENABLED",
        false,
    )
    .await;
    if !enabled() && !enabled_db {
        info!("QTSS_KILL_SWITCH_ENABLED kapalı — kill_switch_loop çıkıyor");
        return;
    }
    let pool_tick = pool.clone();
    let trigger_neg = effective_trigger_neg(&pool).await;
    info!(
        %trigger_neg,
        "kill_switch_loop: günlük realized P&L < eşik ise halt (poll from system_config / env; QTSS_MAX_DRAWDOWN_PCT veya QTSS_KILL_SWITCH_DAILY_LOSS_USDT)"
    );
    loop {
        let poll_secs = resolve_worker_tick_secs(
            &pool_tick,
            "worker",
            "kill_switch_pnl_poll_tick_secs",
            "QTSS_KILL_SWITCH_TICK_SECS",
            60,
            15,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(poll_secs)).await;
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
            persist_halt_flag(&pool, true).await;
        }
    }
}
