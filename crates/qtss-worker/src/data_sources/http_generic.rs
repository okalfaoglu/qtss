//! Config-driven GET/POST (`external_data_sources` row) — PLAN §3 `HttpGenericProvider`.

use std::time::Instant;

use async_trait::async_trait;
use qtss_storage::ExternalDataSourceRow;
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::Client;
use serde_json::{json, Value};

use super::provider::{DataSourceFetchOk, DataSourceProvider};

#[derive(Clone)]
pub struct HttpGenericProvider {
    client: Client,
    source: ExternalDataSourceRow,
}

impl HttpGenericProvider {
    pub fn new(client: Client, source: ExternalDataSourceRow) -> Self {
        Self { client, source }
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
            v["body"] = source.body_json.clone().unwrap_or_else(|| json!({}));
        }
        v
    }

    fn parse_body_json(bytes: &[u8]) -> Option<Value> {
        if bytes.is_empty() {
            return None;
        }
        const MAX: usize = 512 * 1024;
        let slice = if bytes.len() > MAX {
            &bytes[..MAX]
        } else {
            bytes
        };
        if let Ok(v) = serde_json::from_slice::<Value>(slice) {
            return Some(v);
        }
        let lossy = String::from_utf8_lossy(slice).to_string();
        Some(json!({ "_qtss_raw_utf8": lossy }))
    }

    async fn execute_http(&self) -> (Value, Option<Value>, Option<i16>, Option<String>) {
        let method = Self::normalize_method(&self.source.method);
        let mut req = match method {
            "POST" => self.client.post(&self.source.url),
            _ => self.client.get(&self.source.url),
        };
        req = req.header("User-Agent", "qtss-worker/external-fetch");

        if let Some(obj) = self.source.headers_json.as_object() {
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
            let body = self.source.body_json.clone().unwrap_or_else(|| json!({}));
            req.json(&body).send().await
        } else {
            req.send().await
        };

        let req_meta = Self::build_request_json(&self.source, method);

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
            let body_json = Self::parse_body_json(&bytes);
            return (req_meta, body_json, code, Some(err_hint));
        }

        let body_json = Self::parse_body_json(&bytes);
        (req_meta, body_json, code, None)
    }
}

#[async_trait]
impl DataSourceProvider for HttpGenericProvider {
    fn source_key(&self) -> &str {
        &self.source.key
    }

    async fn fetch(&self) -> DataSourceFetchOk {
        let t0 = Instant::now();
        let (request_json, response_json, status_code, error) = self.execute_http().await;
        let ms = t0.elapsed().as_millis().min(9_999_999_999) as u64;
        let meta_json = status_code.map(|c| json!({ "http_status": c }));
        DataSourceFetchOk {
            request_json,
            response_json,
            meta_json,
            error,
            fetch_duration_ms: Some(ms),
        }
    }
}
