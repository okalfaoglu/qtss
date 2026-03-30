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

use sqlx::PgPool;
use tracing::warn;

/// Expire pending AI decisions past `expires_at` every 5 minutes (FAZ 5.2).
pub async fn expire_stale_ai_decisions_loop(pool: PgPool) {
    loop {
        match storage::expire_stale_decisions(&pool).await {
            Ok(n) if n > 0 => tracing::info!(expired = n, "AI decisions marked expired"),
            Err(e) => warn!(%e, "expire_stale_decisions"),
            _ => {}
        }
        tokio::time::sleep(Duration::from_secs(300)).await;
    }
}
