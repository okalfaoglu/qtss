//! `data_snapshots` yazımı — harici HTTP çekimi (eski `external_data_snapshots` kaldırıldı).

use qtss_storage::upsert_data_snapshot;
use sqlx::PgPool;

use super::provider::DataSourceFetchOk;

pub async fn persist_fetch_to_data_snapshot(
    pool: &PgPool,
    source_key: &str,
    out: &DataSourceFetchOk,
) -> Result<(), qtss_storage::StorageError> {
    let err_slice = out.error.as_deref();
    upsert_data_snapshot(
        pool,
        source_key,
        &out.request_json,
        out.response_json.as_ref(),
        out.meta_json.as_ref(),
        err_slice,
    )
    .await?;
    Ok(())
}
