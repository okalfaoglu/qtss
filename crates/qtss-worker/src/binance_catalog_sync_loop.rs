//! Periyodik Binance `exchangeInfo` → `instruments` (spot + USDT-M) senkronu.

use std::time::Duration;

use qtss_binance::{sync_full_binance_catalog, BinanceClient, BinanceClientConfig};
use qtss_storage::{resolve_worker_enabled_flag, resolve_worker_tick_secs, CatalogRepository};
use sqlx::PgPool;
use tracing::{info, warn};

pub async fn binance_catalog_sync_loop(pool: PgPool) {
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "catalog_sync_enabled",
            "QTSS_CATALOG_SYNC_ENABLED",
            true,
        )
        .await;

        if enabled {
            match run_binance_catalog_sync(&pool).await {
                Ok(s) => info!(
                    spot_upserted = s.spot_instruments_upserted,
                    fut_upserted = s.usdt_futures_instruments_upserted,
                    spot_stale = s.spot_deactivated_stale,
                    fut_stale = s.futures_deactivated_stale,
                    "binance catalog sync ok"
                ),
                Err(e) => warn!(%e, "binance catalog sync failed"),
            }
        }

        let secs = resolve_worker_tick_secs(
            &pool,
            "worker",
            "catalog_sync_tick_secs",
            "QTSS_CATALOG_SYNC_TICK_SECS",
            3600,
            300,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

async fn run_binance_catalog_sync(pool: &PgPool) -> anyhow::Result<qtss_binance::CatalogSyncStats> {
    let cfg = BinanceClientConfig::public_mainnet();
    let client = BinanceClient::new(cfg).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let catalog = CatalogRepository::new(pool.clone());
    sync_full_binance_catalog(&client, &catalog)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}
