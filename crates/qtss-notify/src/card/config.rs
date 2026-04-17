//! DB-backed loader for `TierThresholds` and `CategoryThresholds`.
//!
//! The raw `system_config` queries are done via `qtss_storage`'s
//! `resolve_system_f64` / `resolve_system_u64` helpers so we get the
//! same env-override semantics as the rest of the codebase.

use sqlx::PgPool;

use super::category::CategoryThresholds;
use super::tier::TierThresholds;

const MODULE: &str = "notify";

pub async fn load_tier_thresholds(pool: &PgPool) -> TierThresholds {
    let fallback = TierThresholds::FALLBACK;
    TierThresholds {
        orta_min: qtss_storage::resolve_system_f64(
            pool, MODULE, "public_card.tier.orta_min",
            "QTSS_NOTIFY_TIER_ORTA_MIN", fallback.orta_min,
        )
        .await,
        guclu_min: qtss_storage::resolve_system_f64(
            pool, MODULE, "public_card.tier.guclu_min",
            "QTSS_NOTIFY_TIER_GUCLU_MIN", fallback.guclu_min,
        )
        .await,
        cok_guclu_min: qtss_storage::resolve_system_f64(
            pool, MODULE, "public_card.tier.cok_guclu_min",
            "QTSS_NOTIFY_TIER_COK_GUCLU_MIN", fallback.cok_guclu_min,
        )
        .await,
        mukemmel_min: qtss_storage::resolve_system_f64(
            pool, MODULE, "public_card.tier.mukemmel_min",
            "QTSS_NOTIFY_TIER_MUKEMMEL_MIN", fallback.mukemmel_min,
        )
        .await,
    }
}

pub async fn load_category_thresholds(pool: &PgPool) -> CategoryThresholds {
    let fallback = CategoryThresholds::FALLBACK;
    CategoryThresholds {
        crypto_mega_cap_top_n: qtss_storage::resolve_system_u64(
            pool, MODULE, "category.crypto.mega_cap_top_n",
            "QTSS_NOTIFY_CATEGORY_MEGA_CAP_TOP_N",
            fallback.crypto_mega_cap_top_n as u64, 1, 10_000,
        )
        .await as i64,
        crypto_large_cap_top_n: qtss_storage::resolve_system_u64(
            pool, MODULE, "category.crypto.large_cap_top_n",
            "QTSS_NOTIFY_CATEGORY_LARGE_CAP_TOP_N",
            fallback.crypto_large_cap_top_n as u64, 1, 10_000,
        )
        .await as i64,
        crypto_mid_cap_top_n: qtss_storage::resolve_system_u64(
            pool, MODULE, "category.crypto.mid_cap_top_n",
            "QTSS_NOTIFY_CATEGORY_MID_CAP_TOP_N",
            fallback.crypto_mid_cap_top_n as u64, 1, 10_000,
        )
        .await as i64,
        crypto_small_cap_top_n: qtss_storage::resolve_system_u64(
            pool, MODULE, "category.crypto.small_cap_top_n",
            "QTSS_NOTIFY_CATEGORY_SMALL_CAP_TOP_N",
            fallback.crypto_small_cap_top_n as u64, 1, 10_000,
        )
        .await as i64,
        crypto_futures_override: resolve_bool(
            pool, "category.crypto.futures_override",
            "QTSS_NOTIFY_CATEGORY_CRYPTO_FUTURES_OVERRIDE",
            fallback.crypto_futures_override,
        )
        .await,
    }
}

/// Minimal bool resolver — falls back to a string resolver parse so we
/// don't depend on a specific storage helper. The `system_config`
/// tick path stores bools as JSON `true`/`false`; we accept both that
/// and the string forms `"true"/"false"`.
async fn resolve_bool(
    pool: &PgPool,
    key: &str,
    env_key: &str,
    default: bool,
) -> bool {
    let raw = qtss_storage::resolve_system_string(
        pool, MODULE, key, env_key,
        if default { "true" } else { "false" },
    )
    .await;
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => true,
        "false" | "0" | "no" | "off" => false,
        _ => default,
    }
}
