//! qtss-onchain — Faz 7.7 v1.
//!
//! Replaces the legacy "Hat A" pipeline with a small, category-based
//! fetcher set:
//!
//! 1. **Derivatives** (Binance public REST) — funding, OI, long/short
//!    ratio, taker buy/sell. Free, every symbol.
//! 2. **Stablecoin / macro** (DeFiLlama + alternative.me) — stablecoin
//!    market cap delta + Fear & Greed index. Free, market-wide.
//! 3. **Glassnode** (optional, paid) — BTC/ETH cohort metrics
//!    (SOPR, exchange netflow, MVRV). Skipped when no API key.
//!
//! Each fetcher implements [`OnchainCategoryFetcher`] and returns a
//! [`CategoryReading`] in `[-1, +1]` (negative = bearish, positive =
//! bullish). The [`aggregator`] module collapses readings into an
//! [`AggregateOnchain`] consumed by the TBM Onchain pillar via the
//! `qtss-tbm::onchain::OnchainMetricsProvider` trait.
//!
//! Design notes (CLAUDE.md):
//! - All thresholds, weights and category enable flags live in
//!   `system_config` — this crate only knows the dispatch shape.
//! - One match arm per category in [`aggregator::aggregate`]; new
//!   sources just register a new fetcher.

pub mod aggregator;
pub mod cryptoquant;
pub mod derivatives;
pub mod glassnode;
pub mod nansen;
pub mod nansen_enriched;
pub mod stablecoin;
pub mod types;

pub use aggregator::{aggregate, AggregateOnchain, AggregatorWeights};
pub use cryptoquant::CryptoQuantFetcher;
pub use derivatives::BinanceDerivativesFetcher;
pub use glassnode::GlassnodeFetcher;
pub use nansen::{NansenFetcher, NansenTuning};
pub use stablecoin::StablecoinMacroFetcher;
pub use types::{
    CategoryKind, CategoryReading, FetcherError, OnchainCategoryFetcher, OnchainDirection,
};
