//! Multi-provider AI engine (`qtss-ai`): LLM completion traits, parsing, safety, and DB persistence.
//!
//! Cloud and on-prem inference share the same [`providers::AiCompletionProvider`] contract; see
//! `docs/QTSS_MASTER_DEV_GUIDE.md` (FAZ 2–3).

pub mod approval;
pub mod client;
pub mod config;
pub mod context_builder;
pub mod error;
pub mod feedback;
pub mod layers;
pub mod parser;
pub mod providers;
pub mod safety;
pub mod storage;

pub use client::{
    hash_context, run_operational_sweep, run_strategic_sweep, run_tactical_sweep, AiRuntime,
};
pub use config::AiEngineConfig;
pub use error::{AiError, AiResult};

use std::time::Duration;

use qtss_storage::resolve_worker_tick_secs;
use sqlx::PgPool;
use tracing::warn;

/// Expire pending AI decisions past `expires_at` (FAZ 5.2).
/// Interval: `system_config.worker.ai_expire_stale_decisions_tick_secs` (`{"secs":300}`), env `QTSS_AI_EXPIRE_STALE_TICK_SECS`, min **60s** (`QTSS_CONFIG_ENV_OVERRIDES` precedence — see `docs/CONFIG_REGISTRY.md`).
pub async fn expire_stale_ai_decisions_loop(pool: PgPool) {
    loop {
        match storage::expire_stale_decisions(&pool).await {
            Ok(n) if n > 0 => tracing::info!(expired = n, "AI decisions marked expired"),
            Err(e) => warn!(%e, "expire_stale_decisions"),
            _ => {}
        }
        let sleep_secs = resolve_worker_tick_secs(
            &pool,
            "worker",
            "ai_expire_stale_decisions_tick_secs",
            "QTSS_AI_EXPIRE_STALE_TICK_SECS",
            300,
            60,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
    }
}
