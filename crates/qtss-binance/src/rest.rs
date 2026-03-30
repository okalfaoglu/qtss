use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::BinanceCredentials;
use crate::error::BinanceError;
use crate::sign::hmac_hex;

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn build_signed_query(
    mut params: BTreeMap<String, String>,
    secret: &str,
    recv_window_ms: u64,
) -> String {
    params.insert("timestamp".into(), now_ms().to_string());
    params.insert("recvWindow".into(), recv_window_ms.to_string());
    let payload: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");
    let sig = hmac_hex(secret, &payload);
    format!("{}&signature={}", payload, sig)
}

pub struct RestCore {
    pub client: Client,
    pub recv_window_ms: u64,
}

impl RestCore {
    pub fn new(recv_window_ms: u64) -> Result<Self, BinanceError> {
        let client = Client::builder()
            .use_rustls_tls()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self {
            client,
            recv_window_ms,
        })
    }

    pub async fn get_public(
        &self,
        base: &str,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<serde_json::Value, BinanceError> {
        let url = format!("{}{}", base.trim_end_matches('/'), path);
        let mut req = self.client.get(&url);
        if !query.is_empty() {
            req = req.query(query);
        }
        let resp = req.send().await?;
        self.parse_json(resp).await
    }

    /// Tam URL (sorgu dahil) — opsiyonel parametrelerde yaşam süresi kolaylığı için.
    pub async fn get_url(&self, url: &str) -> Result<serde_json::Value, BinanceError> {
        let resp = self.client.get(url).send().await?;
        self.parse_json(resp).await
    }

    pub async fn post_api_key_only(
        &self,
        base: &str,
        path: &str,
        creds: &BinanceCredentials,
    ) -> Result<serde_json::Value, BinanceError> {
        let url = format!("{}{}", base.trim_end_matches('/'), path);
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-MBX-APIKEY",
            HeaderValue::from_str(&creds.api_key)
                .map_err(|e| BinanceError::Other(e.to_string()))?,
        );
        let resp = self.client.post(url).headers(headers).send().await?;
        self.parse_json(resp).await
    }

    pub async fn put_api_key_query(
        &self,
        base: &str,
        path: &str,
        query: &[(&str, &str)],
        creds: &BinanceCredentials,
    ) -> Result<serde_json::Value, BinanceError> {
        let url = format!("{}{}", base.trim_end_matches('/'), path);
        let mut req = self.client.put(url);
        if !query.is_empty() {
            req = req.query(query);
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-MBX-APIKEY",
            HeaderValue::from_str(&creds.api_key)
                .map_err(|e| BinanceError::Other(e.to_string()))?,
        );
        let resp = req.headers(headers).send().await?;
        self.parse_json(resp).await
    }

    pub async fn delete_api_key_query(
        &self,
        base: &str,
        path: &str,
        query: &[(&str, &str)],
        creds: &BinanceCredentials,
    ) -> Result<serde_json::Value, BinanceError> {
        let url = format!("{}{}", base.trim_end_matches('/'), path);
        let mut req = self.client.delete(url);
        if !query.is_empty() {
            req = req.query(query);
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-MBX-APIKEY",
            HeaderValue::from_str(&creds.api_key)
                .map_err(|e| BinanceError::Other(e.to_string()))?,
        );
        let resp = req.headers(headers).send().await?;
        self.parse_json(resp).await
    }

    pub async fn get_signed(
        &self,
        base: &str,
        path: &str,
        params: BTreeMap<String, String>,
        creds: &BinanceCredentials,
    ) -> Result<serde_json::Value, BinanceError> {
        let qs = build_signed_query(params, &creds.api_secret, self.recv_window_ms);
        let url = format!("{}{}?{}", base.trim_end_matches('/'), path, qs);
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-MBX-APIKEY",
            HeaderValue::from_str(&creds.api_key)
                .map_err(|e| BinanceError::Other(e.to_string()))?,
        );
        let resp = self.client.get(url).headers(headers).send().await?;
        self.parse_json(resp).await
    }

    pub async fn post_signed_form(
        &self,
        base: &str,
        path: &str,
        params: BTreeMap<String, String>,
        creds: &BinanceCredentials,
    ) -> Result<serde_json::Value, BinanceError> {
        let body = build_signed_query(params, &creds.api_secret, self.recv_window_ms);
        let url = format!("{}{}", base.trim_end_matches('/'), path);
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-MBX-APIKEY",
            HeaderValue::from_str(&creds.api_key)
                .map_err(|e| BinanceError::Other(e.to_string()))?,
        );
        headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        let resp = self
            .client
            .post(url)
            .headers(headers)
            .body(body)
            .send()
            .await?;
        self.parse_json(resp).await
    }

    pub async fn delete_signed(
        &self,
        base: &str,
        path: &str,
        params: BTreeMap<String, String>,
        creds: &BinanceCredentials,
    ) -> Result<serde_json::Value, BinanceError> {
        let qs = build_signed_query(params, &creds.api_secret, self.recv_window_ms);
        let url = format!("{}{}?{}", base.trim_end_matches('/'), path, qs);
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-MBX-APIKEY",
            HeaderValue::from_str(&creds.api_key)
                .map_err(|e| BinanceError::Other(e.to_string()))?,
        );
        let resp = self.client.delete(url).headers(headers).send().await?;
        self.parse_json(resp).await
    }

    /// İmzalı PUT (nadiren kullanılır; Binance çoğu uçta GET/POST/DELETE).
    #[allow(dead_code)]
    pub async fn put_signed(
        &self,
        base: &str,
        path: &str,
        params: BTreeMap<String, String>,
        creds: &BinanceCredentials,
    ) -> Result<serde_json::Value, BinanceError> {
        let qs = build_signed_query(params, &creds.api_secret, self.recv_window_ms);
        let url = format!("{}{}?{}", base.trim_end_matches('/'), path, qs);
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-MBX-APIKEY",
            HeaderValue::from_str(&creds.api_key)
                .map_err(|e| BinanceError::Other(e.to_string()))?,
        );
        let resp = self.client.put(url).headers(headers).send().await?;
        self.parse_json(resp).await
    }

    async fn parse_json(&self, resp: reqwest::Response) -> Result<serde_json::Value, BinanceError> {
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(BinanceError::from_body(&text).unwrap_or_else(|| {
                BinanceError::Other(format!(
                    "http {}: {}",
                    status.as_u16(),
                    text.chars().take(500).collect::<String>()
                ))
            }));
        }
        if text.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        Ok(serde_json::from_str(&text)?)
    }

    pub fn require_creds(
        creds: &Option<BinanceCredentials>,
    ) -> Result<&BinanceCredentials, BinanceError> {
        creds
            .as_ref()
            .ok_or_else(|| BinanceError::Auth("API anahtarı gerekli".into()))
    }
}
