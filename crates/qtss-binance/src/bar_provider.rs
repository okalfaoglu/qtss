//! [`MarketBarProvider`] implementation for Binance.
//!
//! Wraps the existing [`backfill_binance_public_klines`] function behind
//! the venue-agnostic trait so the worker ingest loop can dispatch to any
//! exchange without hard-coding Binance imports.

use qtss_domain::bar::MarketBarProvider;
use qtss_storage::is_binance_futures_tradable;
use sqlx::PgPool;

use crate::market_bars_backfill::backfill_binance_public_klines;

pub struct BinanceBarProvider {
    pool: PgPool,
}

impl BinanceBarProvider {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl MarketBarProvider for BinanceBarProvider {
    fn exchange_id(&self) -> &str {
        "binance"
    }

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
    > {
        let symbol = symbol.to_string();
        let interval = interval.to_string();
        let segment = segment.to_string();
        Box::pin(async move {
            let n = backfill_binance_public_klines(
                &self.pool,
                &symbol,
                &interval,
                &segment,
                limit,
            )
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            Ok(n)
        })
    }

    fn is_tradable(
        &self,
        symbol: &str,
        segment: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        let symbol = symbol.to_string();
        let segment = segment.to_string();
        Box::pin(async move {
            let seg_db = match segment.trim() {
                "future" | "futures" | "usdt_futures" | "fapi" => "futures",
                _ => return true, // spot always tradable
            };
            if seg_db == "futures" {
                is_binance_futures_tradable(&self.pool, &symbol)
                    .await
                    .unwrap_or(false)
            } else {
                true
            }
        })
    }
}
