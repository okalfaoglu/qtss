//! Coinglass open API — `external_data_sources.key` öneki `coinglass_`.

use sqlx::PgPool;

use super::external_common::run_external_sources_engine;

fn include_coinglass(key: &str) -> bool {
    key.starts_with("coinglass_")
}

pub async fn run(pool: PgPool) {
    run_external_sources_engine(pool, "external_coinglass", include_coinglass).await;
}
