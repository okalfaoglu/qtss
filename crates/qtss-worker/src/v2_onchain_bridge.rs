//! Faz 7.7 / B4 — bridge between the v2 onchain storage table and the
//! TBM Onchain pillar's [`OnchainMetricsProvider`] trait.
//!
//! The TBM detector loop never touches HTTP fetchers directly: it asks
//! a provider for `OnchainMetrics`, which is exactly what this struct
//! synthesises from the latest `qtss_v2_onchain_metrics` row. Stale
//! rows (configurable via `onchain.stale_after_s`) collapse to `None`
//! so the pillar mutes itself instead of feeding old signal.

use async_trait::async_trait;
use chrono::Duration;
use qtss_storage::v2_onchain_metrics::fetch_latest_for_tbm;
use qtss_tbm::onchain::{OnchainMetrics, OnchainMetricsProvider};
use sqlx::PgPool;
use tracing::debug;

pub struct StoredV2OnchainProvider {
    pool: PgPool,
    stale_after_s: i64,
}

impl StoredV2OnchainProvider {
    pub fn new(pool: PgPool, stale_after_s: i64) -> Self {
        Self { pool, stale_after_s }
    }
}

#[async_trait]
impl OnchainMetricsProvider for StoredV2OnchainProvider {
    async fn fetch(&self, symbol: &str) -> Option<OnchainMetrics> {
        let stale = Duration::seconds(self.stale_after_s.max(1));
        match fetch_latest_for_tbm(&self.pool, symbol, stale).await {
            Ok(Some(row)) => Some(OnchainMetrics {
                aggregate_score: Some(row.aggregate_score),
                aggregate_confidence: Some(row.confidence),
                aggregate_direction: Some(row.direction),
                ..Default::default()
            }),
            Ok(None) => None,
            Err(e) => {
                debug!(symbol = %symbol, %e, "onchain bridge fetch failed");
                None
            }
        }
    }
}
