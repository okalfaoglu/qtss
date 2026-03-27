use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BinanceError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("binance api {code}: {msg}")]
    Api { code: i64, msg: String },
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("imza / kimlik: {0}")]
    Auth(String),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Deserialize)]
pub(crate) struct BinanceErrBody {
    pub code: Option<i64>,
    pub msg: Option<String>,
}

impl BinanceError {
    pub(crate) fn from_body(text: &str) -> Option<Self> {
        let v: BinanceErrBody = serde_json::from_str(text).ok()?;
        match (v.code, v.msg) {
            (Some(c), Some(m)) => Some(BinanceError::Api { code: c, msg: m }),
            _ => None,
        }
    }
}
