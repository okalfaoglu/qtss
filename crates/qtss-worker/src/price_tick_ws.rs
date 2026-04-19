//! Faz 9.7.2 — Binance `@bookTicker` WS stream → shared
//! [`qtss_notify::PriceTickStore`].
//!
//! For every open setup on `binance` (venue_class=futures) we subscribe
//! to `<symbol>@bookTicker` via a combined stream. On each frame we
//! update the in-memory store; the setup watcher (9.7.3) reads it to
//! compute live `current_change_pct`, trigger TP/SL transitions, and
//! feed Health Score.
//!
//! CLAUDE.md #2 — all tunables come from `system_config` under the
//! `notify` module, `price_watcher.*` keys.
//! CLAUDE.md #3 — this file is the *adapter*: it parses Binance JSON
//! and pokes the store. Nothing in here knows about setups, tiers,
//! or notification logic.
//!
//! Scope for 9.7.2: futures only (matches existing public WS loops).
//! Spot will be added when we start tracking spot-venue setups.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use qtss_binance::connect_url;
use qtss_notify::{PriceTick, PriceTickStore};
use qtss_storage::{
    list_open_v2_setups, resolve_system_u64, resolve_worker_enabled_flag,
    resolve_worker_tick_secs,
};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;
use std::str::FromStr;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{debug, info, warn};

const MODULE: &str = "notify";
const FUTURES_WS_BASE: &str = "wss://fstream.binance.com/stream?streams=";

/// Parsed bookTicker frame. Public for unit-test visibility only.
#[derive(Debug, Clone)]
pub struct ParsedBookTicker {
    pub symbol: String,
    pub tick: PriceTick,
}

/// Parse a combined-stream `@bookTicker` JSON frame. Accepts both the
/// raw form and the `{stream, data}` wrapper. Returns `None` on any
/// field that fails to parse — the caller just drops the frame.
pub fn parse_book_ticker(text: &str) -> Option<ParsedBookTicker> {
    let v: Value = serde_json::from_str(text).ok()?;
    let data = v.get("data").unwrap_or(&v);
    let symbol = data.get("s")?.as_str()?.to_string();
    let update_id = data.get("u")?.as_u64()?;
    let bid = parse_decimal(data.get("b"))?;
    let ask = parse_decimal(data.get("a"))?;
    // Futures bookTicker includes `T` (transaction time, ms). Fall
    // back to now if absent — the stream is live either way.
    let received_at = data
        .get("T")
        .and_then(|x| x.as_i64())
        .and_then(|ms| chrono::DateTime::<Utc>::from_timestamp_millis(ms))
        .unwrap_or_else(Utc::now);
    Some(ParsedBookTicker {
        symbol,
        tick: PriceTick { bid, ask, update_id, received_at },
    })
}

fn parse_decimal(v: Option<&Value>) -> Option<Decimal> {
    let s = v?.as_str()?;
    Decimal::from_str(s).ok()
}

/// Build the combined-stream URL for `<pair>@bookTicker` paths.
/// Pairs are lower-cased as Binance expects.
fn combined_url(pairs: &[String]) -> String {
    let paths: Vec<String> = pairs.iter().map(|p| format!("{p}@bookTicker")).collect();
    format!("{FUTURES_WS_BASE}{}", paths.join("/"))
}

async fn active_futures_pairs(pool: &PgPool) -> Vec<String> {
    match list_open_v2_setups(pool, None, None).await {
        Ok(rows) => {
            let mut seen: HashSet<String> = HashSet::new();
            for r in rows {
                // Accept both "futures" and the legacy/generic "crypto"
                // venue_class — v2 pipeline currently writes "crypto"
                // for Binance perp setups.
                let vc = r.venue_class.to_ascii_lowercase();
                if r.exchange.eq_ignore_ascii_case("binance")
                    && (vc == "futures" || vc == "crypto")
                {
                    seen.insert(r.symbol.to_ascii_lowercase());
                }
            }
            let mut out: Vec<String> = seen.into_iter().collect();
            out.sort();
            out
        }
        Err(e) => {
            warn!(%e, "price_tick_ws: list_open_v2_setups");
            Vec::new()
        }
    }
}

/// Main loop — reconnects on close / symbol-set change / config
/// disable-enable transitions.
pub async fn price_tick_ws_loop(pool: PgPool, store: Arc<PriceTickStore>) {
    info!("price_tick_ws loop spawned");
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            MODULE,
            "price_watcher.enabled",
            "QTSS_NOTIFY_PRICE_WATCHER_ENABLED",
            false,
        )
        .await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }

        let refresh_secs = resolve_worker_tick_secs(
            &pool,
            MODULE,
            "price_watcher.symbols_refresh_secs",
            "QTSS_NOTIFY_PRICE_WATCHER_REFRESH_SECS",
            60,
            5,
        )
        .await;
        let stale_secs = resolve_worker_tick_secs(
            &pool,
            MODULE,
            "price_watcher.stale_tick_secs",
            "QTSS_NOTIFY_PRICE_WATCHER_STALE_SECS",
            30,
            5,
        )
        .await;
        let max_symbols = resolve_system_u64(
            &pool,
            MODULE,
            "price_watcher.max_active",
            "QTSS_NOTIFY_PRICE_WATCHER_MAX_ACTIVE",
            500,
            1,
            5_000,
        )
        .await as usize;

        let mut pairs = active_futures_pairs(&pool).await;
        if pairs.len() > max_symbols {
            pairs.truncate(max_symbols);
        }
        if pairs.is_empty() {
            debug!("price_tick_ws: no open futures setups");
            tokio::time::sleep(Duration::from_secs(refresh_secs)).await;
            continue;
        }

        let url = combined_url(&pairs);
        info!(streams = pairs.len(), refresh_secs, "price_tick_ws connecting");

        match connect_url(&url).await {
            Ok(mut ws) => {
                let mut refresh_timer =
                    tokio::time::interval(Duration::from_secs(refresh_secs));
                refresh_timer.tick().await; // burn immediate
                let initial_set: HashSet<String> = pairs.iter().cloned().collect();
                loop {
                    tokio::select! {
                        msg = ws.next() => match msg {
                            Some(Ok(Message::Text(t))) => {
                                if let Some(parsed) = parse_book_ticker(&t) {
                                    store.upsert("binance", &parsed.symbol, parsed.tick);
                                }
                            }
                            Some(Ok(Message::Ping(p))) => {
                                let _ = ws.send(Message::Pong(p)).await;
                            }
                            Some(Ok(Message::Close(_))) | None => {
                                warn!("price_tick_ws ws closed");
                                break;
                            }
                            Some(Err(e)) => {
                                warn!(%e, "price_tick_ws ws read");
                                break;
                            }
                            _ => {}
                        },
                        _ = refresh_timer.tick() => {
                            // Purge stale entries & check for symbol-set drift.
                            let purged = store.drain_stale(Utc::now(), stale_secs as i64);
                            if purged > 0 {
                                debug!(purged, "price_tick_ws purged stale ticks");
                            }
                            let fresh = active_futures_pairs(&pool).await;
                            let fresh_set: HashSet<String> = fresh.into_iter().collect();
                            if fresh_set != initial_set {
                                info!(
                                    old = initial_set.len(),
                                    new = fresh_set.len(),
                                    "price_tick_ws symbol set changed; reconnecting"
                                );
                                break;
                            }
                        }
                    }
                }
            }
            Err(e) => warn!(%e, "price_tick_ws connect"),
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_futures_wrapped_frame() {
        let text = r#"{"stream":"btcusdt@bookTicker","data":{"e":"bookTicker","u":12345,"s":"BTCUSDT","b":"82400.10","B":"1.23","a":"82400.50","A":"2.34","T":1713456789000,"E":1713456789001}}"#;
        let p = parse_book_ticker(text).unwrap();
        assert_eq!(p.symbol, "BTCUSDT");
        assert_eq!(p.tick.update_id, 12345);
        assert_eq!(p.tick.bid.to_string(), "82400.10");
        assert_eq!(p.tick.ask.to_string(), "82400.50");
    }

    #[test]
    fn parses_raw_frame_without_wrapper() {
        let text = r#"{"u":99,"s":"ETHUSDT","b":"3000.1","B":"5","a":"3000.2","A":"6"}"#;
        let p = parse_book_ticker(text).unwrap();
        assert_eq!(p.symbol, "ETHUSDT");
        assert_eq!(p.tick.update_id, 99);
    }

    #[test]
    fn rejects_malformed_frame() {
        assert!(parse_book_ticker("not json").is_none());
        assert!(parse_book_ticker(r#"{"u":1,"s":"X"}"#).is_none()); // missing b/a
    }

    #[test]
    fn combined_url_joins_bookticker_paths() {
        let url = combined_url(&["btcusdt".to_string(), "ethusdt".to_string()]);
        assert_eq!(
            url,
            "wss://fstream.binance.com/stream?streams=btcusdt@bookTicker/ethusdt@bookTicker"
        );
    }
}
