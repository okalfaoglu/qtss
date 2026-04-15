//! Faz 7.7 / B4 — bridge between the v2 onchain storage table and the
//! TBM Onchain pillar's [`OnchainMetricsProvider`] trait.
//!
//! The TBM detector loop never touches HTTP fetchers directly: it asks
//! a provider for `OnchainMetrics`, which is exactly what this struct
//! synthesises from the latest `qtss_v2_onchain_metrics` row. Stale
//! rows (configurable via `onchain.stale_after_s`) collapse to `None`
//! so the pillar mutes itself instead of feeding old signal.

use async_trait::async_trait;
use chrono::{Duration, Utc};
use qtss_storage::v2_onchain_metrics::fetch_latest_for_tbm;
use qtss_tbm::onchain::{OnchainMetrics, OnchainMetricsProvider};
use sqlx::PgPool;
use tracing::{debug, warn};

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
            Ok(Some(row)) => {
                // Faz 7.7 / debug — surface staleness + actual values so
                // we can tell a stuck aggregator (same score every tick)
                // from a healthy one. Warn when the row is older than
                // half the stale window (early warning before it mutes
                // entirely via the None branch below).
                let age = Utc::now().signed_duration_since(row.computed_at);
                let age_s = age.num_seconds();
                if age_s > self.stale_after_s / 2 {
                    warn!(
                        symbol = %symbol,
                        age_s,
                        stale_after_s = self.stale_after_s,
                        aggregate_score = row.aggregate_score,
                        direction = %row.direction,
                        "onchain bridge: row getting stale (>50% of stale window) — fetcher may be stuck"
                    );
                } else {
                    debug!(
                        symbol = %symbol,
                        age_s,
                        aggregate_score = row.aggregate_score,
                        confidence = row.confidence,
                        direction = %row.direction,
                        "onchain bridge: fresh metrics"
                    );
                }
                Some(OnchainMetrics {
                    aggregate_score: Some(row.aggregate_score),
                    aggregate_confidence: Some(row.confidence),
                    aggregate_direction: Some(row.direction),
                    ..Default::default()
                })
            },
            Ok(None) => None,
            Err(e) => {
                debug!(symbol = %symbol, %e, "onchain bridge fetch failed");
                None
            }
        }
    }
}
