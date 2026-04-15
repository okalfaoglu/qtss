use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// İşlem/analiz için birincil veri birimi: **tick değil**, zaman damgalı bar.
/// Tick altyapısı ileride ayrı `TickStream` trait’i ile eklenecek.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampBar {
    pub ts: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
}

pub trait TimestampBarFeed: Send {
    fn next_bar(&mut self) -> Option<TimestampBar>;
}

// ---------------------------------------------------------------------------
// Market data provider abstraction (venue-agnostic)
// ---------------------------------------------------------------------------

/// Result of a resumable backfill run.
#[derive(Debug, Clone)]
pub struct BackfillResult {
    pub upserted: i64,
    pub pages: u32,
    pub oldest_ms: Option<u64>,
    pub newest_ms: Option<u64>,
    pub reached_listing: bool,
}

/// Venue-agnostic bar backfill provider. Each exchange adapter (Binance,
/// Bybit, …) implements this so the worker ingest loop doesn't hard-code
/// any exchange. See CLAUDE.md rule #4 (asset-class agnostic core).
///
/// The pool is held inside each implementor (constructor injection) so the
/// trait itself stays free of DB framework types.
pub trait MarketBarProvider: Send + Sync {
    /// Human-readable exchange name (e.g. "binance").
    fn exchange_id(&self) -> &str;

    /// Fetch up to `limit` bars for the given symbol/interval/segment from
    /// the exchange REST API and upsert them into `market_bars`. Returns
    /// the number of rows upserted.
    fn backfill_bars(
        &self,
        symbol: &str,
        interval: &str,
        segment: &str,
        limit: i64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<i64, Box<dyn std::error::Error + Send + Sync>>>
            + Send
            + '_>,
    >;

    /// Resumable backfill: fetch bars backwards from `resume_end_ms`.
    /// Returns (upserted, pages, oldest_ms, newest_ms, reached_listing).
    /// Default delegates to `backfill_bars` ignoring resume cursor.
    fn backfill_bars_resumable(
        &self,
        symbol: &str,
        interval: &str,
        segment: &str,
        limit: i64,
        resume_end_ms: Option<u64>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        BackfillResult,
                        Box<dyn std::error::Error + Send + Sync>,
                    >,
                > + Send
                + '_,
        >,
    >;

    /// Check whether the symbol is tradable on this exchange/segment.
    fn is_tradable(
        &self,
        symbol: &str,
        segment: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>>;
}
