//! Binance FAPI / `futures/data/*` — `external_data_sources.key` öneki `binance_`.

use sqlx::PgPool;

use super::external_common::run_external_sources_engine;

fn include_binance(key: &str) -> bool {
    key.starts_with("binance_")
}

pub async fn run(pool: PgPool) {
    run_external_sources_engine(pool, "external_binance", include_binance).await;
}
