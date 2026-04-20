//! Faz 14.A — scheduled background loops for symbol intelligence.
//!
//! Two independent tokio loops:
//!
//! 1. `symbol_catalog_refresh_loop` — çalıştığı borsalar için
//!    `symbol-catalog-refresh` binary'sini invoke eder. CoinGecko
//!    ücretsiz tier'da 30 req/dk; yeni sembol ilk çağrıda kaçabilir,
//!    bir sonraki round kapatır. Interval `config_schema`'dan
//!    (`symbol_intel.catalog_refresh_interval_s`) okunur, default 24h.
//!
//! 2. `market_regime_tick_loop` — `market-regime-tick` binary'sini
//!    çağırır; default 1h.
//!
//! Binary'yi spawn ediyoruz çünkü A2 tarafındaki CoinGecko HTTP + DB
//! upsert mantığı 500 satır civarı; inline import etmek yerine
//! subprocess'le tetiklemek daha ucuz (CLAUDE.md #1 — tek sorumluluk).

use std::process::Stdio;
use std::time::Duration;

use sqlx::PgPool;
use tokio::process::Command;
use tracing::{info, warn};

use qtss_storage::config_tick::{resolve_system_u64, resolve_worker_enabled_flag};

fn binary_path(name: &str) -> String {
    // Aynı process'te çalışıyoruz; target dizini executable'ın yanında.
    // Env override (production deploy için): QTSS_SYMBOL_INTEL_BIN_DIR.
    if let Ok(dir) = std::env::var("QTSS_SYMBOL_INTEL_BIN_DIR") {
        return format!("{dir}/{name}");
    }
    // Current exe'nin parent dizinine bak (target/release veya target/debug).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join(name).to_string_lossy().into_owned();
        }
    }
    name.to_string()
}

async fn run_binary(name: &str, env: &[(&str, &str)]) {
    let path = binary_path(name);
    let mut cmd = Command::new(&path);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    match cmd.status().await {
        Ok(s) if s.success() => info!(bin = %name, "finished ok"),
        Ok(s) => warn!(bin = %name, status = ?s.code(), "non-zero exit"),
        Err(e) => warn!(bin = %name, error = %e, "spawn failed"),
    }
}

pub async fn symbol_catalog_refresh_loop(pool: PgPool) {
    let enabled = resolve_worker_enabled_flag(
        &pool,
        "symbol_intel",
        "catalog_refresh_enabled",
        "QTSS_SYMBOL_CATALOG_REFRESH_ENABLED",
        true,
    )
    .await;
    if !enabled {
        info!("symbol_catalog_refresh_loop disabled");
        return;
    }

    let interval_s = resolve_system_u64(
        &pool,
        "symbol_intel",
        "catalog_refresh_interval_s",
        "",
        86_400, // 24h
        300,
        7 * 86_400,
    )
    .await;
    let exchange =
        std::env::var("SYMBOL_REFRESH_EXCHANGE").unwrap_or_else(|_| "binance".into());

    info!(interval_s, exchange, "symbol_catalog_refresh_loop starting");
    loop {
        run_binary(
            "symbol-catalog-refresh",
            &[("SYMBOL_REFRESH_EXCHANGE", exchange.as_str())],
        )
        .await;
        tokio::time::sleep(Duration::from_secs(interval_s)).await;
    }
}

pub async fn market_regime_tick_loop(pool: PgPool) {
    let enabled = resolve_worker_enabled_flag(
        &pool,
        "symbol_intel",
        "regime_tick_enabled",
        "QTSS_MARKET_REGIME_TICK_ENABLED",
        true,
    )
    .await;
    if !enabled {
        info!("market_regime_tick_loop disabled");
        return;
    }

    let interval_s = resolve_system_u64(
        &pool,
        "symbol_intel",
        "regime_tick_interval_s",
        "",
        3_600, // 1h
        60,
        86_400,
    )
    .await;
    let exchange = std::env::var("REGIME_EXCHANGE").unwrap_or_else(|_| "binance".into());

    info!(interval_s, exchange, "market_regime_tick_loop starting");
    loop {
        run_binary("market-regime-tick", &[("REGIME_EXCHANGE", exchange.as_str())]).await;
        tokio::time::sleep(Duration::from_secs(interval_s)).await;
    }
}
