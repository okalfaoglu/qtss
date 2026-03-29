//! Nansen Token Screener → `nansen_snapshots` + `data_snapshots` (`DataSourceProvider` + `nansen_persist`).

use std::time::Duration;

use std::sync::atomic::{AtomicBool, Ordering};

use qtss_common::log_critical;
use reqwest::Client;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::data_sources::nansen_persist::persist_nansen_token_screener_fetch;
use crate::data_sources::nansen_token_screener_provider::NansenTokenScreenerProvider;
use crate::data_sources::provider::DataSourceProvider;

static LOGGED_MISSING_NANSEN_KEY: AtomicBool = AtomicBool::new(false);

pub async fn nansen_token_screener_loop(pool: PgPool) {
    let secs: u64 = std::env::var("NANSEN_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800)
        .max(60);

    let client = match Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, "nansen: reqwest client oluşturulamadı");
            return;
        }
    };

    let provider = NansenTokenScreenerProvider::new(client, pool.clone());
    let insufficient_sleep: u64 = std::env::var("NANSEN_INSUFFICIENT_CREDITS_SLEEP_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600)
        .max(secs);

    info!(%secs, "nansen token_screener döngüsü (DataSourceProvider)");

    loop {
        let mut next_sleep = secs;

        if std::env::var("NANSEN_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .is_none()
        {
            if !LOGGED_MISSING_NANSEN_KEY.swap(true, Ordering::SeqCst) {
                warn!(
                    "NANSEN_API_KEY tanımsız veya boş — token screener çalışmıyor; \
                     systemd EnvironmentFile / .env ile anahtarı verin ve servisi yeniden başlatın"
                );
            }
            tokio::time::sleep(Duration::from_secs(next_sleep)).await;
            continue;
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
