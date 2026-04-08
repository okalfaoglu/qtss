//! Multi-provider AI engine (`qtss-ai`): LLM completion traits, parsing, safety, and DB persistence.
//!
//! Cloud and on-prem inference share the same [`providers::AiCompletionProvider`] contract; see
//! `docs/QTSS_MASTER_DEV_GUIDE.md` (FAZ 2–3).

pub mod approval;
pub mod strategy_provider;

pub use strategy_provider::{
    AiStrategyProvider, AiStrategyProviderConfig, AiStrategyVerdict, LlmAdvisor,
};

pub use approval::AiDecisionNotifySnapshot;
pub mod circuit_breaker;
pub mod client;
pub mod config;
pub mod context_builder;
pub mod error;
pub mod feedback;
pub mod layers;
pub mod notify_telegram_config;
pub mod parser;
/// Binance / provider secret resolution — crate-private; use `providers` and `config` entry points.
mod provider_secrets;
pub mod providers;
pub mod safety;
pub mod storage;

pub use client::{
    hash_context, run_operational_sweep, run_strategic_sweep, run_tactical_sweep, AiRuntime,
};
pub use feedback::{outcome_stats_for_prompt, record_decision_outcome};
pub use storage::mirror_approval_request_outcome_to_linked_ai_decisions;
pub use storage::{
    notify_ai_tactical_executor_wake, AI_TACTICAL_EXECUTOR_WAKE_NOTIFY_CHANNEL,
};
pub use config::AiEngineConfig;
pub use error::{AiError, AiResult};
pub use notify_telegram_config::load_notify_config_merged;

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
