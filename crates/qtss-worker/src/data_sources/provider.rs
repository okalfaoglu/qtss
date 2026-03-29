use async_trait::async_trait;
use serde_json::Value;

/// One fetch outcome → `data_snapshots` upsert. Put HTTP status in `meta_json` (e.g. `http_status`).
/// `error` is set on transport failure or non-success HTTP (worker still persists a row).
#[derive(Debug, Clone)]
pub struct DataSourceFetchOk {
    pub request_json: Value,
    pub response_json: Option<Value>,
    pub meta_json: Option<Value>,
    pub error: Option<String>,
}

/// Pluggable collectors (`external_fetch`, future Nansen adapter, …).
#[async_trait]
pub trait DataSourceProvider: Send + Sync {
    /// Stable id — for HTTP rows this is `external_data_sources.key`.
    fn source_key(&self) -> &str;

    async fn fetch(&self) -> DataSourceFetchOk;
}
