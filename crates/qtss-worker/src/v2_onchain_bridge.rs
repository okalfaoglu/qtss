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
use qtss_storage::v2_onchain_metrics::{fetch_latest_for_tbm_bucket, TfBucket};
use qtss_tbm::onchain::{OnchainMetrics, OnchainMetricsProvider};
use sqlx::PgPool;
use tracing::{debug, warn};

pub struct StoredV2OnchainProvider {
    pool: PgPool,
    stale_after_s: i64,
    /// Cadence cutoff (seconds) at which a caller's analysis TF is
    /// routed to the `ltf` bucket instead of `htf` (Faz 7.7 / P29c).
    ltf_cadence_s: u64,
}

impl StoredV2OnchainProvider {
    #[allow(dead_code)] // legacy constructor kept for external callers / tests
    pub fn new(pool: PgPool, stale_after_s: i64) -> Self {
        Self::with_ltf_cadence(pool, stale_after_s, 3600)
    }

    pub fn with_ltf_cadence(pool: PgPool, stale_after_s: i64, ltf_cadence_s: u64) -> Self {
        Self { pool, stale_after_s, ltf_cadence_s }
    }

    fn bucket_for_tf(&self, tf_s: u64) -> TfBucket {
        // tf_s == 0 means "caller did not declare a TF" — fall back to
        // the full blend to preserve legacy semantics.
        if tf_s > 0 && tf_s <= self.ltf_cadence_s {
            TfBucket::Ltf
        } else {
            TfBucket::Htf
        }
    }

    async fn fetch_bucket(&self, symbol: &str, bucket: TfBucket) -> Option<OnchainMetrics> {
        let stale = Duration::seconds(self.stale_after_s.max(1));
        match fetch_latest_for_tbm_bucket(&self.pool, symbol, bucket, stale).await {
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
                        bucket = bucket.as_str(),
                        age_s,
                        stale_after_s = self.stale_after_s,
                        aggregate_score = row.aggregate_score,
                        direction = %row.direction,
                        "onchain bridge: row getting stale (>50% of stale window) — fetcher may be stuck"
                    );
                } else {
                    debug!(
                        symbol = %symbol,
                        bucket = bucket.as_str(),
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
                debug!(symbol = %symbol, bucket = bucket.as_str(), %e, "onchain bridge fetch failed");
                None
            }
        }
    }
}

#[async_trait]
impl OnchainMetricsProvider for StoredV2OnchainProvider {
    async fn fetch(&self, symbol: &str) -> Option<OnchainMetrics> {
        // Legacy entry point: no TF context → full-blend htf bucket.
        self.fetch_bucket(symbol, TfBucket::Htf).await
    }

    async fn fetch_for_tf(&self, symbol: &str, tf_s: u64) -> Option<OnchainMetrics> {
        self.fetch_bucket(symbol, self.bucket_for_tf(tf_s)).await
    }
}
