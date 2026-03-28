//! `external_data_sources` satırlarına göre periyodik HTTP çekimi → `external_data_snapshots`.

use std::time::Duration;

use qtss_storage::{
    external_snapshot_age_secs, list_enabled_external_sources, upsert_external_snapshot,
    ExternalDataSourceRow,
};
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::Client;
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{info, warn};

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

fn normalize_method(m: &str) -> &'static str {
    match m.trim().to_ascii_uppercase().as_str() {
        "POST" => "POST",
        _ => "GET",
    }
}

fn build_request_json(source: &ExternalDataSourceRow, method: &str) -> Value {
    let mut v = json!({
        "url": source.url,
        "method": method,
    });
    if method == "POST" {
        v["body"] = source
            .body_json
            .clone()
            .unwrap_or_else(|| json!({}));
    }
    v
}

async fn execute_fetch(client: &Client, source: &ExternalDataSourceRow) -> (Value, Option<Value>, Option<i16>, Option<String>) {
    let method = normalize_method(&source.method);
    let mut req = match method {
        "POST" => client.post(&source.url),
        _ => client.get(&source.url),
    };
    req = req.header("User-Agent", "qtss-worker/external-fetch");

    if let Some(obj) = source.headers_json.as_object() {
        for (k, val) in obj {
            if let Some(s) = val.as_str() {
                if let (Ok(name), Ok(v)) = (
                    HeaderName::from_bytes(k.as_bytes()),
                    HeaderValue::try_from(s),
                ) {
                    req = req.header(name, v);
                }
            }
        }
    }

    let resp = if method == "POST" {
        let body = source.body_json.clone().unwrap_or_else(|| json!({}));
        req.json(&body).send().await
    } else {
        req.send().await
    };

    let req_meta = build_request_json(source, method);

    let Ok(resp) = resp else {
        return (
            req_meta,
            None,
            None,
            Some("HTTP isteği gönderilemedi veya zaman aşımı".into()),
        );
    };

    let status = resp.status();
    let code = Some(status.as_u16() as i16);
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return (
                req_meta,
                None,
                code,
                Some(format!("yanıt gövdesi okunamadı: {e}")),
            );
        }
    };

    if !status.is_success() {
        let err_hint = format!("HTTP {}", status.as_u16());
        let body_json = parse_body_json(&bytes);
        return (req_meta, body_json, code, Some(err_hint));
    }

    let body_json = parse_body_json(&bytes);
    (req_meta, body_json, code, None)
}

fn parse_body_json(bytes: &[u8]) -> Option<Value> {
    if bytes.is_empty() {
        return None;
    }
    const MAX: usize = 512 * 1024;
    let slice = if bytes.len() > MAX { &bytes[..MAX] } else { bytes };
    if let Ok(v) = serde_json::from_slice::<Value>(slice) {
        return Some(v);
    }
    let lossy = String::from_utf8_lossy(slice).to_string();
    Some(json!({ "_qtss_raw_utf8": lossy }))
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

            let (req_meta, resp_json, status_code, err) = execute_fetch(&client, &s).await;
            let err_slice = err.as_deref();
            if let Err(e) = upsert_external_snapshot(
                &pool,
                &s.key,
                &req_meta,
                resp_json.as_ref(),
                status_code,
                err_slice,
            )
            .await
            {
                warn!(%e, key = %s.key, "external_data_snapshots upsert");
            } else if err_slice.is_none() {
                tracing::debug!(key = %s.key, "external_fetch snapshot güncellendi");
            } else {
                warn!(key = %s.key, ?err, "external_fetch HTTP hata — snapshot yine yazıldı");
            }
        }

        tokio::time::sleep(poll).await;
    }
}
