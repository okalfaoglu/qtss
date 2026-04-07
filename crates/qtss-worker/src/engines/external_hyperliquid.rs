//! Hyperliquid `POST /info` — `hl_*` ve `hl_meta_asset_ctxs` (migration / confluence anahtarı).

use sqlx::PgPool;

use super::external_common::run_external_sources_engine;

fn include_hyperliquid(key: &str) -> bool {
    key.starts_with("hl_") || key == "hl_meta_asset_ctxs"
}

pub async fn run(pool: PgPool) {
    run_external_sources_engine(pool, "external_hyperliquid", include_hyperliquid, false).await;
}
