//! Faz 9.0.2 — Feature Store writer + source registry.
//!
//! Worker tarafı:
//!   * [`FeatureStore::write_for_detection`] — `ConfluenceSource` registry
//!     üzerinde iterate eder, enabled source'ların `extract` dönüşünü
//!     `qtss_features_snapshot`'e upsert eder.
//!   * [`DbSourceQuery`] — `SourceQuery` trait'inin DB impl'i; extractor'lar
//!     data_snapshots / regime / tbm tablolarına buradan ulaşır.
//!
//! Registry static: `FEATURE_SOURCES`. Yeni source eklemek → yeni modül
//! + registry'ye bir satır (CLAUDE.md #1).

use async_trait::async_trait;
use qtss_confluence::{ConfluenceSource, SourceContext, SourceQuery};
use qtss_storage::{resolve_worker_enabled_flag, resolve_worker_tick_secs};
use serde_json::Value;
use sqlx::PgPool;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::feature_sources;

/// Static registry: extend here when a new source lands.
pub static FEATURE_SOURCES: &[&dyn ConfluenceSource] = &[
    &feature_sources::wyckoff::WyckoffSource,
    &feature_sources::derivatives::DerivativesSource,
    &feature_sources::orderbook::OrderbookSource,
    // Faz 9.8.AI-Yol2 — structural-detector feature extractors.
    &feature_sources::elliott::ElliottSource,
    &feature_sources::harmonic::HarmonicSource,
    &feature_sources::classical::ClassicalSource,
    &feature_sources::gap::GapSource,
    &feature_sources::range::RangeSource,
    &feature_sources::tbm::TbmSource,
];

/// DB-backed impl of the confluence `SourceQuery` port.
pub struct DbSourceQuery<'a> {
    pool: &'a PgPool,
}

impl<'a> DbSourceQuery<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl<'a> SourceQuery for DbSourceQuery<'a> {
    async fn data_snapshot(&self, key: &str) -> Option<Value> {
        sqlx::query_scalar::<_, Option<Value>>(
            "SELECT response_json FROM data_snapshots WHERE source_key = $1",
        )
        .bind(key)
        .fetch_optional(self.pool)
        .await
        .ok()
        .flatten()
        .flatten()
    }

    async fn latest_regime(
        &self,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
    ) -> Option<Value> {
        // Regime snapshots live under different table names depending on
        // deployment; prefer `qtss_v2_regime_snapshots` if present.
        sqlx::query_scalar::<_, Option<Value>>(
            r#"SELECT raw_json FROM qtss_v2_regime_snapshots
                WHERE exchange=$1 AND symbol=$2 AND timeframe=$3
                ORDER BY computed_at DESC LIMIT 1"#,
        )
        .bind(exchange)
        .bind(symbol)
        .bind(timeframe)
        .fetch_optional(self.pool)
        .await
        .ok()
        .flatten()
        .flatten()
    }

    async fn latest_tbm(&self, exchange: &str, symbol: &str, timeframe: &str) -> Option<Value> {
        sqlx::query_scalar::<_, Option<Value>>(
            r#"SELECT raw_json FROM qtss_v2_tbm_metrics
                WHERE exchange=$1 AND symbol=$2 AND timeframe=$3
                ORDER BY computed_at DESC LIMIT 1"#,
        )
        .bind(exchange)
        .bind(symbol)
        .bind(timeframe)
        .fetch_optional(self.pool)
        .await
        .ok()
        .flatten()
        .flatten()
    }
}

pub struct FeatureStore<'a> {
    pub pool: &'a PgPool,
}

impl<'a> FeatureStore<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    pub async fn enabled(pool: &PgPool) -> bool {
        resolve_worker_enabled_flag(
            pool,
            "ai",
            "feature_store.enabled",
            "QTSS_FEATURE_STORE_ENABLED",
            true,
        )
        .await
    }

    async fn source_enabled(pool: &PgPool, source_key: &str) -> bool {
        let cfg_key = format!("feature_store.sources.{source_key}.enabled");
        // tick-loaders re-use the bool resolver; env key is per-source.
        let env_key = format!("QTSS_FS_SRC_{}_ON", source_key.to_uppercase());
        resolve_worker_enabled_flag(pool, "ai", &cfg_key, &env_key, true).await
    }

    async fn current_spec_version(pool: &PgPool) -> i32 {
        // Reuse tick_secs resolver as a simple int resolver (>=1 clamp ok).
        resolve_worker_tick_secs(
            pool,
            "ai",
            "feature_store.spec_version",
            "QTSS_FEATURE_SPEC_VERSION",
            1,
            1,
        )
        .await as i32
    }

    pub async fn write_for_detection(
        &self,
        detection_id: Uuid,
        setup_id: Option<Uuid>,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
        event_bar_ms: Option<i64>,
        raw_detection: &Value,
    ) -> Result<usize, sqlx::Error> {
        if !Self::enabled(self.pool).await {
            return Ok(0);
        }
        let spec_version = Self::current_spec_version(self.pool).await;
        let ctx = SourceContext {
            exchange,
            symbol,
            timeframe,
            detection_id: Some(detection_id),
            setup_id,
            event_bar_ms,
            raw_detection,
        };
        let query = DbSourceQuery::new(self.pool);

        let mut written = 0usize;
        for src in FEATURE_SOURCES {
            if !Self::source_enabled(self.pool, src.key()).await {
                continue;
            }
            let snap = match src.extract(&ctx, &query).await {
                Some(s) => s,
                None => {
                    debug!(source = src.key(), "feature_store: source skipped");
                    continue;
                }
            };
            if snap.features.is_empty() {
                continue;
            }
            let (features_json, meta_json) = snap.clone().into_json();
            // Unique index `uq_features_snap_detection_source` was
            // rewritten non-partial in migration 0120 so PG can match
            // the ON CONFLICT target cleanly.
            let res = sqlx::query(
                r#"
                INSERT INTO qtss_features_snapshot
                    (detection_id, setup_id, exchange, symbol, timeframe,
                     source, feature_spec_version, features_json,
                     computed_at_bar_ms, meta_json)
                VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
                ON CONFLICT (detection_id, source, feature_spec_version)
                    DO UPDATE SET features_json = EXCLUDED.features_json,
                                  meta_json     = EXCLUDED.meta_json,
                                  computed_at   = now()
                "#,
            )
            .bind(detection_id)
            .bind(setup_id)
            .bind(exchange)
            .bind(symbol)
            .bind(timeframe)
            .bind(snap.source)
            .bind(spec_version)
            .bind(features_json)
            .bind(event_bar_ms)
            .bind(meta_json)
            .execute(self.pool)
            .await;
            match res {
                Ok(_) => written += 1,
                Err(e) => warn!(%e, source = snap.source, "feature_snapshot insert"),
            }
        }
        Ok(written)
    }
}
