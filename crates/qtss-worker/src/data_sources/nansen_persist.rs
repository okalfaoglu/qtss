//! Nansen token screener: `nansen_snapshots` + `data_snapshots` dual-write (`DataSourceFetchOk`).

use qtss_storage::{upsert_data_snapshot, upsert_nansen_snapshot};
use sqlx::PgPool;

use super::persist::meta_with_fetch_duration;
use super::provider::DataSourceFetchOk;
use super::registry::NANSEN_TOKEN_SCREENER_DATA_KEY;

const SNAPSHOT_KIND: &str = "token_screener";

pub async fn persist_nansen_token_screener_fetch(
    pool: &PgPool,
    out: &DataSourceFetchOk,
) -> Result<(), qtss_storage::StorageError> {
    let err = out.error.as_deref();
    let meta = meta_with_fetch_duration(out.meta_json.clone(), out.fetch_duration_ms);
    upsert_nansen_snapshot(
        pool,
        SNAPSHOT_KIND,
        &out.request_json,
        out.response_json.as_ref(),
        meta.as_ref(),
        err,
    )
    .await?;
    upsert_data_snapshot(
        pool,
        NANSEN_TOKEN_SCREENER_DATA_KEY,
        &out.request_json,
        out.response_json.as_ref(),
        meta.as_ref(),
        err,
    )
    .await?;
    Ok(())
}
