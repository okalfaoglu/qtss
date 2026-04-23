//! Analysis engine crate: `engine_symbols` → `trading_range` + `signal_dashboard` DB snapshots.
//!
//! Confluence persistence stays in `qtss-worker` (`confluence` module); inject via [`ConfluencePersist`].
//! See `docs/QTSS_CURSOR_DEV_GUIDE.md` §9.1 item 5.

mod engine_loop;
mod error;
pub mod scorer;

use async_trait::async_trait;

pub use error::AnalysisError;
pub use scorer::{
    load_config as load_confluence_config, ConfluenceConfig, ConfluenceScorer,
    ConfluenceSnapshot, ConfluenceVerdict,
};
use chrono::{DateTime, Utc};
use qtss_storage::EngineSymbolRow;
use serde_json::Value;
use sqlx::PgPool;

pub use engine_loop::engine_analysis_loop;

#[async_trait]
pub trait ConfluencePersist: Send + Sync {
    async fn compute_and_persist(
        &self,
        pool: &PgPool,
        t: &EngineSymbolRow,
        dash_payload: &Value,
        last_bar_open_time: DateTime<Utc>,
        bar_count: i32,
    ) -> Result<(), AnalysisError>;
}
