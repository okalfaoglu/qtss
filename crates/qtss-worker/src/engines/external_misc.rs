//! Diğer GET/POST kaynakları (ör. DeFi Llama, özel URL) — Binance / Coinglass / HL filtreleri dışında kalan satırlar.

use sqlx::PgPool;

use super::external_common::run_external_sources_engine;

fn include_misc(key: &str) -> bool {
    !key.starts_with("binance_")
        && !key.starts_with("coinglass_")
        && !(key.starts_with("hl_") || key == "hl_meta_asset_ctxs")
}

pub async fn run(pool: PgPool) {
    run_external_sources_engine(pool, "external_misc", include_misc, false).await;
}
