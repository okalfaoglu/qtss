//! Shared trait + value types for the onchain fetchers.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Category buckets — kept tiny on purpose. Adding a new bucket means
/// extending [`CategoryKind`], the [`crate::aggregator`] dispatch and
/// the `system_config` weights row. Three is enough for v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CategoryKind {
    /// Derivatives microstructure (funding, OI, taker flow…). Always
    /// available, every symbol.
    Derivatives,
    /// Macro liquidity (stablecoin supply, fear & greed). Market-wide,
    /// not symbol-specific.
    Stablecoin,
    /// On-chain cohort metrics (SOPR, MVRV, netflow). BTC/ETH only,
    /// requires Glassnode key.
    Chain,
}

/// Coarse direction emitted by a fetcher. Mapped to [`OnchainMetricsProvider`]
/// downstream so the TBM caller can do its bottom/top reconciliation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnchainDirection {
    Long,
    Short,
    Neutral,
}

impl OnchainDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Long => "long",
            Self::Short => "short",
            Self::Neutral => "neutral",
        }
    }
}

/// One fetcher's verdict. Score is normalised to `[-1, +1]` so the
/// aggregator can blend categories without knowing each source's units.
/// Confidence in `[0, 1]` lets a noisy/stale fetcher down-weight itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryReading {
    pub category: CategoryKind,
    /// Bullish positive, bearish negative. Clamped to `[-1, 1]`.
    pub score: f64,
    /// Source confidence in `[0, 1]`. Multiplied into the aggregator
    /// weight for this category.
    pub confidence: f64,
    /// Optional direction hint. When `None` the aggregator infers it
    /// from the sign of `score`.
    pub direction: Option<OnchainDirection>,
    /// Free-form details (one short string per signal) — surfaced in
    /// the TBM detection's `raw_meta.details` for ops debugging.
    pub details: Vec<String>,
}

impl CategoryReading {
    pub fn neutral(category: CategoryKind) -> Self {
        Self {
            category,
            score: 0.0,
            confidence: 0.0,
            direction: Some(OnchainDirection::Neutral),
            details: Vec::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum FetcherError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("decode: {0}")]
    Decode(String),
    #[error("disabled: {0}")]
    Disabled(&'static str),
    #[error("unsupported symbol: {0}")]
    UnsupportedSymbol(String),
}

/// Fetcher contract — async because everything here hits the network.
/// Implementations must be `Send + Sync` so the worker can spawn them
/// in parallel via `tokio::join!`.
#[async_trait]
pub trait OnchainCategoryFetcher: Send + Sync {
    /// Stable identifier (also used as `system_config` namespace).
    fn name(&self) -> &'static str;

    /// Which bucket this fetcher feeds.
    fn category(&self) -> CategoryKind;

    /// Fetch current reading for `symbol` (e.g. `"BTCUSDT"`).
    async fn fetch(&self, symbol: &str) -> Result<CategoryReading, FetcherError>;
}
