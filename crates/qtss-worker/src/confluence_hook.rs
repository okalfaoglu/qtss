//! Bridges `qtss_analysis::ConfluencePersist` to worker `confluence::compute_and_persist`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_analysis::ConfluencePersist;
use qtss_storage::EngineSymbolRow;
use serde_json::Value;
use sqlx::PgPool;

pub struct WorkerConfluenceHook;

#[async_trait]
impl ConfluencePersist for WorkerConfluenceHook {
    async fn compute_and_persist(
        &self,
        pool: &PgPool,
        t: &EngineSymbolRow,
        dash_payload: &Value,
        last_bar_open_time: DateTime<Utc>,
        bar_count: i32,
    ) -> Result<(), String> {
        crate::confluence::compute_and_persist(pool, t, dash_payload, last_bar_open_time, bar_count)
            .await
    }
}
