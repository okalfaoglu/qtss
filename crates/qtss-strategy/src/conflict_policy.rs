//! TA vs on-chain conflict → skip veya yarım boy (`strategy.signal_filter_on_conflict`).

use qtss_storage::resolve_system_string;
use sqlx::PgPool;

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ConflictSizePolicy {
    Skip,
    Half,
}

/// `half` / `half_size` → yarım miktar; aksi halde atla.
pub async fn conflict_size_policy_from_db(pool: &PgPool) -> ConflictSizePolicy {
    let raw = resolve_system_string(
        pool,
        "strategy",
        "signal_filter_on_conflict",
        "QTSS_SIGNAL_FILTER_ON_CONFLICT",
        "skip",
    )
    .await;
    match raw.trim().to_lowercase().as_str() {
        "half" | "half_size" => ConflictSizePolicy::Half,
        _ => ConflictSizePolicy::Skip,
    }
}
