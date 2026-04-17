//! Shared in-memory price tick store — populated by the worker's
//! `@bookTicker` WS loop (Faz 9.7.2), read by the setup watcher
//! (Faz 9.7.3) and Smart Target AI (Faz 9.7.4).
//!
//! Keyed by `(exchange, symbol_upper)`; a single entry holds the
//! latest bid/ask/update_id/timestamp. Stale entries are cleared on
//! demand via [`PriceTickStore::drain_stale`].
//!
//! CLAUDE.md #3: stream details (bookTicker parsing, reconnect) live
//! in `qtss-worker`; this crate just exposes the type the watcher
//! needs — no venue-specific knowledge here.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Single bookTicker-style snapshot. `update_id` comes from Binance
/// (`u` field on `@bookTicker`) and is monotonic per symbol; we drop
/// out-of-order frames on upsert.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PriceTick {
    pub bid: Decimal,
    pub ask: Decimal,
    pub update_id: u64,
    pub received_at: DateTime<Utc>,
}

impl PriceTick {
    pub fn mid(&self) -> Decimal {
        (self.bid + self.ask) / Decimal::from(2)
    }

    /// Spread in basis points of mid price. Returns `0.0` if mid is zero.
    pub fn spread_bps(&self) -> f64 {
        let mid = self.mid();
        if mid.is_zero() {
            return 0.0;
        }
        let spread = self.ask - self.bid;
        (spread / mid * Decimal::from(10_000))
            .to_f64()
            .unwrap_or(0.0)
    }

    /// Milliseconds since receive.
    pub fn age_ms(&self, now: DateTime<Utc>) -> i64 {
        (now - self.received_at).num_milliseconds().max(0)
    }
}

/// `(exchange_lower, symbol_upper)` map key. We normalise on write so
/// callers can pass whatever case.
pub type PriceKey = (String, String);

fn make_key(exchange: &str, symbol: &str) -> PriceKey {
    (
        exchange.to_ascii_lowercase(),
        symbol.to_ascii_uppercase(),
    )
}

/// Thread-safe price tick cache. All methods are non-async; the map
/// is protected by `std::sync::RwLock` — writes come from exactly one
/// WS task, reads come from many setup-watcher tasks.
#[derive(Debug, Default)]
pub struct PriceTickStore {
    inner: RwLock<HashMap<PriceKey, PriceTick>>,
}

impl PriceTickStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Insert or replace. Drops the frame if `update_id` is older than
    /// the existing entry (defensive — Binance combined stream can
    /// reorder frames under load).
    pub fn upsert(&self, exchange: &str, symbol: &str, tick: PriceTick) -> bool {
        let key = make_key(exchange, symbol);
        let mut g = self.inner.write().expect("price tick store poisoned");
        if let Some(prev) = g.get(&key) {
            if tick.update_id <= prev.update_id {
                return false;
            }
        }
        g.insert(key, tick);
        true
    }

    pub fn get(&self, exchange: &str, symbol: &str) -> Option<PriceTick> {
        let key = make_key(exchange, symbol);
        self.inner
            .read()
            .expect("price tick store poisoned")
            .get(&key)
            .copied()
    }

    pub fn len(&self) -> usize {
        self.inner.read().expect("price tick store poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove entries older than `max_age` seconds. Returns the count
    /// purged — caller decides whether to log.
    pub fn drain_stale(&self, now: DateTime<Utc>, max_age_secs: i64) -> usize {
        let threshold = now - chrono::Duration::seconds(max_age_secs.max(0));
        let mut g = self.inner.write().expect("price tick store poisoned");
        let before = g.len();
        g.retain(|_, t| t.received_at >= threshold);
        before - g.len()
    }

    /// Snapshot of all keys currently held — useful for metrics /
    /// debug dumps. Does not expose the tick values.
    pub fn keys(&self) -> Vec<PriceKey> {
        self.inner
            .read()
            .expect("price tick store poisoned")
            .keys()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn tick(bid: Decimal, ask: Decimal, update_id: u64, ts: DateTime<Utc>) -> PriceTick {
        PriceTick { bid, ask, update_id, received_at: ts }
    }

    #[test]
    fn upsert_and_get_round_trip() {
        let store = PriceTickStore::new();
        let now = Utc::now();
        assert!(store.upsert("binance", "BTCUSDT", tick(dec!(82_400), dec!(82_401), 10, now)));
        let t = store.get("BINANCE", "btcusdt").unwrap();
        assert_eq!(t.bid, dec!(82_400));
        assert_eq!(t.update_id, 10);
    }

    #[test]
    fn out_of_order_frame_dropped() {
        let store = PriceTickStore::new();
        let now = Utc::now();
        assert!(store.upsert("binance", "BTCUSDT", tick(dec!(1), dec!(2), 20, now)));
        // older update_id → dropped
        assert!(!store.upsert("binance", "BTCUSDT", tick(dec!(3), dec!(4), 19, now)));
        let t = store.get("binance", "BTCUSDT").unwrap();
        assert_eq!(t.update_id, 20);
        assert_eq!(t.bid, dec!(1));
    }

    #[test]
    fn mid_and_spread() {
        let t = tick(dec!(100), dec!(100.5), 1, Utc::now());
        assert_eq!(t.mid(), dec!(100.25));
        // spread = 0.5, mid = 100.25, bps = 0.5 / 100.25 * 10000 ≈ 49.875
        assert!((t.spread_bps() - 49.875).abs() < 0.01);
    }

    #[test]
    fn drain_stale_purges_old_entries() {
        let store = PriceTickStore::new();
        let now = Utc::now();
        let old = now - chrono::Duration::seconds(120);
        store.upsert("binance", "OLD", tick(dec!(1), dec!(2), 1, old));
        store.upsert("binance", "NEW", tick(dec!(1), dec!(2), 1, now));
        let purged = store.drain_stale(now, 60);
        assert_eq!(purged, 1);
        assert!(store.get("binance", "OLD").is_none());
        assert!(store.get("binance", "NEW").is_some());
    }
}
