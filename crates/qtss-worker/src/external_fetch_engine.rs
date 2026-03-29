//! `external_data_sources` satırlarına göre periyodik HTTP çekimi → `data_snapshots`.

use std::time::Duration;

use qtss_storage::{external_snapshot_age_secs, list_enabled_external_sources};
use reqwest::Client;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::data_sources::http_generic::HttpGenericProvider;
use crate::data_sources::persist::persist_fetch_to_data_snapshot;
use crate::data_sources::provider::DataSourceProvider;

fn external_fetch_enabled() -> bool {
    match std::env::var("QTSS_EXTERNAL_FETCH")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("0") | Some("false") | Some("no") | Some("off") => false,
        _ => true,
    }
}

fn poll_interval_secs() -> u64 {
    std::env::var("QTSS_EXTERNAL_FETCH_POLL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30)
        .max(10)
}

fn tick_floor_secs(s: i32) -> i64 {
    (s as i64).max(30)
}

pub async fn external_fetch_loop(pool: PgPool) {
    if !external_fetch_enabled() {
        info!("QTSS_EXTERNAL_FETCH kapalı — external_data_snapshots güncellenmiyor");
        return;
    }

    let poll = Duration::from_secs(poll_interval_secs());
    let client = match Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, "external_fetch: reqwest client");
            return;
        }
    };

    info!(poll_secs = poll.as_secs(), "external_fetch döngüsü");

    loop {
        let sources = match list_enabled_external_sources(&pool).await {
            Ok(s) => s,
            Err(e) => {
                warn!(%e, "external_data_sources listesi");
                tokio::time::sleep(poll).await;
                continue;
            }
        };

        for s in sources {
            let tick = tick_floor_secs(s.tick_secs);
            let stale = match external_snapshot_age_secs(&pool, &s.key).await {
                Ok(None) => true,
                Ok(Some(age)) => age >= tick,
                Err(e) => {
                    warn!(%e, key = %s.key, "snapshot yaşı okunamadı");
                    true
                }
            };
            if !stale {
                continue;
            }

            let provider = HttpGenericProvider::new(client.clone(), s);
            let key = provider.source_key();
            let out = provider.fetch().await;
            let err_opt = out.error.clone();
            if let Err(e) = persist_fetch_to_data_snapshot(&pool, key, &out).await {
                warn!(%e, key = %key, "data_snapshots upsert");
            } else if err_opt.is_none() {
                tracing::debug!(key = %key, "external_fetch snapshot güncellendi");
            } else {
                warn!(key = %key, ?err_opt, "external_fetch HTTP hata — snapshot yine yazıldı");
            }
        }

        tokio::time::sleep(poll).await;
    }
}
