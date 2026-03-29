//! Funding eşiği — spot+futures çifti için iskelet (dev guide ADIM 7, §3.7).
//!
//! Binance premium `data_snapshots` (`binance_premium_*usdt`) üzerinden funding okur; gerçek çift bacaklı emir
//! için ayrı hesap/policy gerekir — şimdilik sinyal logu.

use std::sync::Arc;
use std::time::Duration;

use qtss_common::is_trading_halted;
use qtss_storage::fetch_data_snapshot;
use sqlx::PgPool;
use tracing::{info, warn};

fn tick_secs() -> u64 {
    std::env::var("QTSS_ARB_FUNDING_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300)
        .max(60)
}

fn threshold() -> f64 {
    std::env::var("QTSS_ARB_FUNDING_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0001)
}

pub async fn run(pool: PgPool, _gateway: Arc<dyn qtss_execution::ExecutionGateway>) {
    let tick = Duration::from_secs(tick_secs());
    let base = std::env::var("QTSS_ARB_FUNDING_SYMBOL_BASE")
        .unwrap_or_else(|_| "btc".into())
        .trim()
        .to_lowercase();
    let key = format!("binance_premium_{base}usdt");
    info!(%key, poll_secs = tick.as_secs(), "arb_funding izleme");
    loop {
        tokio::time::sleep(tick).await;
        if is_trading_halted() {
            continue;
        }
        let row = match fetch_data_snapshot(&pool, &key).await {
            Ok(r) => r,
            Err(e) => {
                warn!(%e, "arb_funding fetch_data_snapshot");
                continue;
            }
        };
        let Some(j) = row.and_then(|r| r.response_json) else {
            continue;
        };
        let fr = j
            .get("lastFundingRate")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let th = threshold();
        if fr.abs() < th {
            continue;
        }
        if fr > th {
            info!(funding_rate = fr, "arb_funding: pozitif funding — spot AL + futures SHORT sinyali (uygulama: gateway + segment)");
        } else {
            info!(funding_rate = fr, "arb_funding: negatif funding — ters yön sinyali");
        }
    }
}
