//! Nansen Token Screener → `nansen_snapshots` (global). `NANSEN_API_KEY` yoksa döngü atlanır.

use std::time::Duration;

use std::sync::atomic::{AtomicBool, Ordering};

use qtss_common::log_critical;
use qtss_nansen::post_token_screener;
use qtss_storage::{upsert_data_snapshot, upsert_nansen_snapshot};
use reqwest::Client;
use serde_json::json;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::data_sources::registry::NANSEN_TOKEN_SCREENER_DATA_KEY;
use crate::nansen_query::{nansen_api_base, token_screener_body};

static LOGGED_MISSING_NANSEN_KEY: AtomicBool = AtomicBool::new(false);

const SNAPSHOT_KIND: &str = "token_screener";

/// Son başarılı yanıttan satır sayısı tahmini (API şeması değişebilir).
fn response_token_count(v: &serde_json::Value) -> Option<usize> {
    v.get("data")
        .and_then(|d| d.as_array())
        .map(|a| a.len())
        .or_else(|| v.get("tokens").and_then(|t| t.as_array()).map(|a| a.len()))
        .or_else(|| v.get("results").and_then(|r| r.as_array()).map(|a| a.len()))
}

pub async fn nansen_token_screener_loop(pool: PgPool) {
    // Varsayılan 30 dk — kredi tüketimini düşük tutar; sık tarama için NANSEN_TICK_SECS düşürün.
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

    let base = nansen_api_base();
    info!(%base, %secs, "nansen token_screener döngüsü");

    loop {
        let Some(api_key) = std::env::var("NANSEN_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
        else {
            if !LOGGED_MISSING_NANSEN_KEY.swap(true, Ordering::SeqCst) {
                warn!(
                    "NANSEN_API_KEY tanımsız veya boş — token screener çalışmıyor; \
                     systemd EnvironmentFile / .env ile anahtarı verin ve servisi yeniden başlatın"
                );
            }
            tokio::time::sleep(Duration::from_secs(secs)).await;
            continue;
        };

        let body = token_screener_body(&pool).await;
        let insufficient_sleep: u64 = std::env::var("NANSEN_INSUFFICIENT_CREDITS_SLEEP_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600)
            .max(secs);
        let mut next_sleep = secs;
        match post_token_screener(&client, &base, &api_key, &body).await {
            Ok((json, meta)) => {
                let meta_json = json!({
                    "credits_used": meta.credits_used,
                    "credits_remaining": meta.credits_remaining,
                    "rate_limit_remaining": meta.rate_limit_remaining,
                    "response_token_count_hint": response_token_count(&json),
                });
                if let Err(e) = upsert_nansen_snapshot(
                    &pool,
                    SNAPSHOT_KIND,
                    &body,
                    Some(&json),
                    Some(&meta_json),
                    None,
                )
                .await
                {
                    warn!(%e, "nansen_snapshots upsert");
                } else {
                    info!("nansen token_screener snapshot güncellendi");
                }
                if let Err(e) = upsert_data_snapshot(
                    &pool,
                    NANSEN_TOKEN_SCREENER_DATA_KEY,
                    &body,
                    Some(&json),
                    Some(&meta_json),
                    None,
                )
                .await
                {
                    warn!(%e, "data_snapshots nansen upsert");
                }
            }
            Err(e) => {
                if e.is_insufficient_credits() {
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
                warn!(%e, "nansen token_screener isteği başarısız");
                let err_str = e.to_string();
                if let Err(e2) = upsert_nansen_snapshot(
                    &pool,
                    SNAPSHOT_KIND,
                    &body,
                    None,
                    None,
                    Some(err_str.as_str()),
                )
                .await
                {
                    warn!(%e2, "nansen_snapshots hata satırı yazılamadı");
                }
                if let Err(e3) = upsert_data_snapshot(
                    &pool,
                    NANSEN_TOKEN_SCREENER_DATA_KEY,
                    &body,
                    None,
                    None,
                    Some(err_str.as_str()),
                )
                .await
                {
                    warn!(%e3, "data_snapshots nansen error upsert");
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(next_sleep)).await;
    }
}
