//! Nansen Token Screener → `nansen_snapshots` + `data_snapshots` (`DataSourceProvider` + `nansen_persist`).

use std::time::Duration;

use std::sync::atomic::{AtomicBool, Ordering};

use qtss_common::log_critical;
use qtss_storage::{
    resolve_nansen_loop_default_on, resolve_system_string, resolve_worker_enabled_flag, resolve_worker_tick_secs,
    SystemConfigRepository,
};
use reqwest::Client;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::data_sources::nansen_persist::persist_nansen_token_screener_fetch;
use crate::data_sources::nansen_token_screener_provider::NansenTokenScreenerProvider;
use crate::data_sources::provider::DataSourceProvider;

static LOGGED_MISSING_NANSEN_KEY: AtomicBool = AtomicBool::new(false);

pub async fn nansen_token_screener_loop(pool: PgPool) {
    let client = match Client::builder().timeout(Duration::from_secs(120)).build() {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, "nansen: reqwest client oluşturulamadı");
            return;
        }
    };

    let provider = NansenTokenScreenerProvider::new(client, pool.clone());

    info!("nansen token_screener döngüsü (DataSourceProvider; tick: worker.nansen_token_screener_tick_secs / NANSEN_TICK_SECS)");

    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "nansen_enabled",
            "QTSS_NANSEN_ENABLED",
            true,
        )
        .await;
        let secs = resolve_worker_tick_secs(
            &pool,
            "worker",
            "nansen_token_screener_tick_secs",
            "NANSEN_TICK_SECS",
            1800,
            60,
        )
        .await;
        let insufficient_sleep = resolve_worker_tick_secs(
            &pool,
            "worker",
            "nansen_insufficient_credits_sleep_secs",
            "NANSEN_INSUFFICIENT_CREDITS_SLEEP_SECS",
            3600,
            60,
        )
        .await
        .max(secs);

        let mut next_sleep = secs;

        if !enabled {
            tokio::time::sleep(Duration::from_secs(next_sleep)).await;
            continue;
        }

        let screener_on = resolve_nansen_loop_default_on(
            &pool,
            "nansen_token_screener_loop_enabled",
            "NANSEN_TOKEN_SCREENER_ENABLED",
        )
        .await;
        if !screener_on {
            tokio::time::sleep(Duration::from_secs(next_sleep)).await;
            continue;
        }

        let sys = SystemConfigRepository::new(pool.clone());
        let api_key = sys
            .get("worker", "nansen_api_key")
            .await
            .ok()
            .flatten()
            .and_then(|r| r.value.get("value").and_then(|x| x.as_str()).map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("NANSEN_API_KEY").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
        let api_base = resolve_system_string(&pool, "worker", "nansen_api_base", "NANSEN_API_BASE", "https://api.nansen.ai").await;

        if api_key.is_none() {
            if !LOGGED_MISSING_NANSEN_KEY.swap(true, Ordering::SeqCst) {
                warn!(
                    "NANSEN_API_KEY tanımsız veya boş — token screener çalışmıyor; \
                     systemd EnvironmentFile / .env ile anahtarı verin ve servisi yeniden başlatın"
                );
            }
            tokio::time::sleep(Duration::from_secs(next_sleep)).await;
            continue;
        }
        if !api_base.trim().is_empty() {
            std::env::set_var("NANSEN_API_BASE", api_base);
        }
        if let Some(k) = api_key.as_ref() {
            std::env::set_var("NANSEN_API_KEY", k);
        }

        let out = provider.fetch().await;
        let insufficient = out
            .meta_json
            .as_ref()
            .and_then(|m| m.get("nansen_insufficient_credits"))
            .and_then(|x| x.as_bool())
            .unwrap_or(false);

        if insufficient {
            log_critical(
                "qtss_worker_nansen",
                "Nansen kredisi tükendi (Insufficient credits). Token screener çağrıları başarısız; \
                 Nansen planında kredi yükleyin veya NANSEN_TICK_SECS / NANSEN_INSUFFICIENT_CREDITS_SLEEP_SECS ile aralığı artırın.",
            );
            next_sleep = insufficient_sleep;
            warn!(
                sleep_secs = next_sleep,
                "nansen: kredi yetersiz — bir sonraki deneme için uzun bekleme"
            );
        }

        if out.error.is_some() && !insufficient {
            warn!(?out.error, "nansen token_screener isteği başarısız");
        }

        if let Err(e) = persist_nansen_token_screener_fetch(&pool, &out).await {
            warn!(%e, "nansen dual persist (nansen_snapshots / data_snapshots)");
        } else if out.error.is_none() {
            info!("nansen token_screener snapshot güncellendi");
        }

        tokio::time::sleep(Duration::from_secs(next_sleep)).await;
    }
}

// ADIM 3 — genişletilmiş Nansen HTTP döngüleri `nansen_extended.rs` içinde; buradan re-export.
pub use crate::nansen_extended::{
    nansen_flow_intel_loop, nansen_holdings_loop, nansen_netflows_loop,
    nansen_perp_leaderboard_loop, nansen_perp_screener_loop, nansen_perp_trades_loop,
    nansen_smart_money_dex_trades_loop, nansen_tgm_dex_trades_loop, nansen_tgm_flows_loop,
    nansen_tgm_holders_loop, nansen_tgm_indicators_loop, nansen_tgm_perp_positions_loop,
    nansen_tgm_perp_trades_tgm_loop, nansen_tgm_token_information_loop,
    nansen_whale_perp_aggregate_loop, nansen_who_bought_loop,
};
