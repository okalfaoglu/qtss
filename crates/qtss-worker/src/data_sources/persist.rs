//! `data_snapshots` yazımı — harici HTTP çekimi (eski `external_data_snapshots` kaldırıldı).

use qtss_storage::upsert_data_snapshot;
use serde_json::{json, Value};
use sqlx::PgPool;

use super::provider::DataSourceFetchOk;

pub(super) fn meta_with_fetch_duration(meta: Option<Value>, fetch_duration_ms: Option<u64>) -> Option<Value> {
    match (meta, fetch_duration_ms) {
        (None, None) => None,
        (Some(m), None) => Some(m),
        (None, Some(ms)) => Some(json!({ "qtss_fetch_duration_ms": ms })),
        (Some(Value::Object(mut o)), Some(ms)) => {
            o.insert("qtss_fetch_duration_ms".into(), json!(ms));
            Some(Value::Object(o))
        }
        (Some(other), Some(ms)) => Some(json!({
            "qtss_embedded_meta": other,
            "qtss_fetch_duration_ms": ms
        })),
    }
}

pub async fn persist_fetch_to_data_snapshot(
    pool: &PgPool,
    source_key: &str,
    out: &DataSourceFetchOk,
) -> Result<(), qtss_storage::StorageError> {
    let err_slice = out.error.as_deref();
    let meta = meta_with_fetch_duration(out.meta_json.clone(), out.fetch_duration_ms);
    upsert_data_snapshot(
        pool,
        source_key,
        &out.request_json,
        out.response_json.as_ref(),
        meta.as_ref(),
        err_slice,
    )
    .await?;
    Ok(())
}
