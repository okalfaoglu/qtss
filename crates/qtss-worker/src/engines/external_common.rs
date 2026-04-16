//! `external_data_sources` → `data_snapshots` ortak döngü gövdesi (HTTP generic).
//! Aile bazlı motorlar yalnızca `key_filter` ile ayrılır.

use std::time::Duration;

use qtss_storage::{
    external_snapshot_age_secs, is_binance_futures_tradable, list_enabled_external_sources,
    resolve_worker_enabled_flag, resolve_worker_tick_secs,
};
use reqwest::Client;
use sqlx::PgPool;
use tracing::{info, trace, warn};

use crate::data_sources::http_generic::HttpGenericProvider;
use crate::data_sources::persist::persist_fetch_to_data_snapshot;
use crate::data_sources::provider::DataSourceProvider;

fn tick_floor_secs(s: i32) -> i64 {
    (s as i64).max(30)
}

/// Suffix after `binance_taker_` / `binance_premium_` / `binance_open_interest_` / `binance_ls_ratio_` /
/// `binance_funding_rate_`, uppercased for catalog lookup (e.g. `btcusdt` → `BTCUSDT`).
/// Other `binance_*` keys return `None`.
fn native_pair_upper_for_binance_metric_key(key: &str) -> Option<String> {
    const PREFIXES: &[&str] = &[
        "binance_taker_",
        "binance_premium_",
        "binance_open_interest_",
        "binance_ls_ratio_",
        "binance_funding_rate_",
    ];
    for prefix in PREFIXES {
        if let Some(rest) = key.strip_prefix(prefix) {
            let rest = rest.trim();
            if rest.is_empty() {
                return None;
            }
            return Some(rest.to_uppercase());
        }
    }
    None
}

async fn external_fetch_engine_poll(pool: &PgPool) -> Duration {
    let secs = resolve_worker_tick_secs(
        pool,
        "worker",
        "external_fetch_poll_tick_secs",
        "QTSS_EXTERNAL_FETCH_POLL_SECS",
        30,
        10,
    )
    .await;
    Duration::from_secs(secs)
}

/// Tek API ailesi: `key_filter` ile satırlar seçilir; hepsi aynı `data_snapshots` tablosuna yazılır.
/// `binance_futures_catalog_gate`: Binance metric keys (`binance_premium_*`, `binance_open_interest_*`, …)
/// için çekimden önce `instruments` kataloğunda USDT-M tradable doğrulanır (HTTP 400 gürültüsünü keser).
pub async fn run_external_sources_engine(
    pool: PgPool,
    engine_label: &'static str,
    key_filter: fn(&str) -> bool,
    binance_futures_catalog_gate: bool,
) {
    let initial_poll = external_fetch_engine_poll(&pool).await;
    let client = match Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, engine = engine_label, "reqwest client");
            return;
        }
    };

    info!(
        engine = engine_label,
        poll_secs = initial_poll.as_secs(),
        "external HTTP engine poll interval (system_config worker.external_fetch_poll_tick_secs or QTSS_EXTERNAL_FETCH_POLL_SECS)"
    );

    loop {
        // Legacy hard-kill switch remains env-only (bootstrapping / emergency).
        // Everything else is controlled via system_config/DB with env fallback.
        match std::env::var("QTSS_EXTERNAL_FETCH")
            .ok()
            .as_deref()
            .map(str::trim)
        {
            Some("0") | Some("false") | Some("no") | Some("off") => {
                tokio::time::sleep(Duration::from_secs(30)).await;
                continue;
            }
            _ => {}
        }
        if !resolve_worker_enabled_flag(
            &pool,
            "worker",
            "external_fetch_enabled",
            "QTSS_EXTERNAL_FETCH_ENABLED",
            true,
        )
        .await
        {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let poll = external_fetch_engine_poll(&pool).await;

        let sources = match list_enabled_external_sources(&pool).await {
            Ok(s) => s,
            Err(e) => {
                warn!(%e, engine = engine_label, "external_data_sources listesi");
                tokio::time::sleep(poll).await;
                continue;
            }
        };

        for s in sources {
            if !key_filter(&s.key) {
                continue;
            }

            let tick = tick_floor_secs(s.tick_secs);
            let stale = match external_snapshot_age_secs(&pool, &s.key).await {
                Ok(None) => true,
                Ok(Some(age)) => age >= tick,
                Err(e) => {
                    warn!(%e, key = %s.key, engine = engine_label, "snapshot yaşı okunamadı");
                    true
                }
            };
            if !stale {
                continue;
            }

            if binance_futures_catalog_gate {
                if let Some(native) = native_pair_upper_for_binance_metric_key(&s.key) {
                    let tradable = match is_binance_futures_tradable(&pool, &native).await {
                        Ok(v) => v,
                        Err(e) => {
                            warn!(%e, key = %s.key, engine = engine_label, "Binance tradable check failed; skip fetch");
                            false
                        }
                    };
                    if !tradable {
                        trace!(
                            key = %s.key,
                            native = %native,
                            engine = engine_label,
                            "skip external fetch: not Binance USDT-M tradable"
                        );
                        continue;
                    }
                }
            }

            let provider = HttpGenericProvider::new(client.clone(), s);
            let key = provider.source_key().to_string();
            let out = provider.fetch().await;
            let err_opt = out.error.clone();
            if let Err(e) = persist_fetch_to_data_snapshot(&pool, &key, &out).await {
                warn!(%e, key = %key, engine = engine_label, "data_snapshots upsert");
            } else if err_opt.is_none() {
                tracing::debug!(key = %key, engine = engine_label, "snapshot güncellendi");
            } else {
                warn!(
                    key = %key,
                    engine = engine_label,
                    ?err_opt,
                    "HTTP hata — snapshot yine yazıldı"
                );
            }
        }

        tokio::time::sleep(poll).await;
    }
}
