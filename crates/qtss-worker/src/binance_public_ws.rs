//! Faz 9.0.0 — Binance USDT-M public WS streams for AI confluence features.
//!
//! İki döngü:
//!   * [`liquidation_stream_loop`] — `@forceOrder` (rare events, keep rolling
//!     window) → `data_snapshots(binance_liquidations_<pair>)`.
//!   * [`aggtrade_cvd_loop`] — `@aggTrade` (high volume) → 60 sn bucket'lı
//!     CVD → `data_snapshots(binance_cvd_<pair>)`.
//!
//! Her iki worker da aktif USDT-M futures sembollerini alır, combined stream
//! URL'si kurar (`wss://fstream.binance.com/stream?streams=...`), mesaj
//! parse'ı sonrası sembol başına buffer tutar ve tick aralığında upsert eder.
//!
//! CLAUDE.md #2: tick/window/bucket boyutları `system_config` tablosundan
//! okunur (`binance_ws.*` anahtarları).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use qtss_binance::connect_url;
use qtss_storage::{
    list_enabled_engine_symbols, resolve_worker_enabled_flag, resolve_worker_tick_secs,
    upsert_data_snapshot,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{debug, info, warn};

const WS_BASE: &str = "wss://fstream.binance.com/stream?streams=";
const MAX_LIQ_EVENTS: usize = 5000;
const MAX_CVD_BUCKETS: usize = 1440; // 24h @ 60s

fn active_futures_pairs_lower(symbols: &[qtss_storage::EngineSymbolRow]) -> Vec<String> {
    symbols
        .iter()
        .filter(|s| s.segment == "futures" && s.enabled)
        .map(|s| s.symbol.to_lowercase())
        .collect()
}

fn combined_url(pairs: &[String], stream_kind: &str) -> String {
    let paths: Vec<String> = pairs
        .iter()
        .map(|p| format!("{p}@{stream_kind}"))
        .collect();
    format!("{WS_BASE}{}", paths.join("/"))
}

// ---------------------------------------------------------------------------
// Liquidations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
struct LiquidationEvent {
    ts_ms: i64,
    side: String,
    qty: f64,
    price: f64,
    avg_price: f64,
    status: String,
}

fn parse_liquidation(text: &str) -> Option<(String, LiquidationEvent)> {
    let v: Value = serde_json::from_str(text).ok()?;
    let data = v.get("data").or(Some(&v))?;
    let o = data.get("o")?;
    let symbol = o.get("s")?.as_str()?.to_string();
    let ev = LiquidationEvent {
        ts_ms: o.get("T").and_then(|x| x.as_i64()).unwrap_or(0),
        side: o.get("S").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        qty: o
            .get("q")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        price: o
            .get("p")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        avg_price: o
            .get("ap")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        status: o.get("X").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    };
    Some((symbol, ev))
}

type LiqBuffer = Arc<Mutex<HashMap<String, Vec<LiquidationEvent>>>>;

async fn flush_liquidations(
    pool: &PgPool,
    buf: &LiqBuffer,
    window_ms: i64,
) -> Result<(), qtss_storage::StorageError> {
    let now_ms = Utc::now().timestamp_millis();
    let cutoff = now_ms - window_ms;
    let mut guard = buf.lock().await;
    for (sym, events) in guard.iter_mut() {
        events.retain(|e| e.ts_ms >= cutoff);
        if events.len() > MAX_LIQ_EVENTS {
            let drop_n = events.len() - MAX_LIQ_EVENTS;
            events.drain(0..drop_n);
        }
        let key = format!("binance_liquidations_{}", sym.to_lowercase());
        let response = json!({
            "events": events,
            "window_ms": window_ms,
            "count": events.len(),
            "computed_at_ms": now_ms,
        });
        let request = json!({"stream": format!("{}@forceOrder", sym.to_lowercase())});
        upsert_data_snapshot(pool, &key, &request, Some(&response), None, None).await?;
    }
    Ok(())
}

pub async fn liquidation_stream_loop(pool: PgPool) {
    info!("binance_liquidation_stream loop spawned");
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "binance_ws",
            "liquidation_stream.enabled",
            "QTSS_BINANCE_LIQ_WS_ENABLED",
            true,
        )
        .await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }

        let flush_secs = resolve_worker_tick_secs(
            &pool,
            "binance_ws",
            "liquidation_stream.flush_secs",
            "QTSS_BINANCE_LIQ_FLUSH_SECS",
            30,
            5,
        )
        .await;
        let window_secs = resolve_worker_tick_secs(
            &pool,
            "binance_ws",
            "liquidation_stream.window_secs",
            "QTSS_BINANCE_LIQ_WINDOW_SECS",
            3600,
            60,
        )
        .await;

        let symbols = match list_enabled_engine_symbols(&pool).await {
            Ok(v) => v,
            Err(e) => {
                warn!(%e, "liquidation_stream: engine_symbols read");
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        };
        let pairs = active_futures_pairs_lower(&symbols);
        if pairs.is_empty() {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        let url = combined_url(&pairs, "forceOrder");
        info!(streams = pairs.len(), %flush_secs, %window_secs, "liquidation_stream connecting");

        let buf: LiqBuffer = Arc::new(Mutex::new(HashMap::new()));
        let flush_buf = buf.clone();
        let flush_pool = pool.clone();
        let flush_window_ms = (window_secs as i64) * 1000;
        let flusher = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(flush_secs));
            tick.tick().await; // burn immediate
            loop {
                tick.tick().await;
                if let Err(e) = flush_liquidations(&flush_pool, &flush_buf, flush_window_ms).await {
                    warn!(%e, "liquidation_stream flush");
                }
            }
        });

        match connect_url(&url).await {
            Ok(mut ws) => {
                info!("liquidation_stream ws connected");
                while let Some(msg) = ws.next().await {
                    match msg {
                        Ok(Message::Text(t)) => {
                            if let Some((sym, ev)) = parse_liquidation(&t) {
                                let mut g = buf.lock().await;
                                g.entry(sym).or_default().push(ev);
                            }
                        }
                        Ok(Message::Ping(p)) => {
                            let _ = ws.send(Message::Pong(p)).await;
                        }
                        Ok(Message::Close(_)) => break,
                        Err(e) => {
                            warn!(%e, "liquidation_stream ws read");
                            break;
                        }
                        _ => {}
                    }
                }
                warn!("liquidation_stream ws closed");
            }
            Err(e) => warn!(%e, "liquidation_stream connect"),
        }
        flusher.abort();
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

// ---------------------------------------------------------------------------
// Aggregated trade → CVD
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct CvdBucket {
    bucket_ts_ms: i64,
    buy_qty: f64,
    sell_qty: f64,
    trades: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct CvdBucketOut {
    bucket_ts_ms: i64,
    buy_qty: f64,
    sell_qty: f64,
    trades: u64,
    delta: f64,
    cvd: f64,
}

type CvdBuffer = Arc<Mutex<HashMap<String, Vec<CvdBucket>>>>;

fn bucket_of(ts_ms: i64, bucket_secs: i64) -> i64 {
    let b = bucket_secs * 1000;
    (ts_ms / b) * b
}

async fn ingest_aggtrade(text: &str, buf: &CvdBuffer, bucket_secs: i64) {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };
    let data = v.get("data").unwrap_or(&v);
    let symbol = match data.get("s").and_then(|x| x.as_str()) {
        Some(s) => s.to_string(),
        None => return,
    };
    let ts_ms = data.get("T").and_then(|x| x.as_i64()).unwrap_or(0);
    let qty: f64 = data
        .get("q")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    if qty == 0.0 {
        return;
    }
    // m=true → buyer is maker → trade is a SELL initiated by taker.
    let is_maker_buy = data.get("m").and_then(|x| x.as_bool()).unwrap_or(false);
    let bts = bucket_of(ts_ms, bucket_secs);
    let mut g = buf.lock().await;
    let v = g.entry(symbol).or_default();
    if let Some(last) = v.last_mut() {
        if last.bucket_ts_ms == bts {
            if is_maker_buy {
                last.sell_qty += qty;
            } else {
                last.buy_qty += qty;
            }
            last.trades += 1;
            return;
        }
    }
    let mut nb = CvdBucket {
        bucket_ts_ms: bts,
        ..Default::default()
    };
    if is_maker_buy {
        nb.sell_qty += qty;
    } else {
        nb.buy_qty += qty;
    }
    nb.trades = 1;
    v.push(nb);
}

async fn flush_cvd(
    pool: &PgPool,
    buf: &CvdBuffer,
    window_ms: i64,
    bucket_secs: i64,
) -> Result<(), qtss_storage::StorageError> {
    let now_ms = Utc::now().timestamp_millis();
    let cutoff = now_ms - window_ms;
    let mut guard = buf.lock().await;
    for (sym, buckets) in guard.iter_mut() {
        buckets.retain(|b| b.bucket_ts_ms >= cutoff);
        if buckets.len() > MAX_CVD_BUCKETS {
            let drop_n = buckets.len() - MAX_CVD_BUCKETS;
            buckets.drain(0..drop_n);
        }
        let mut cvd_run = 0.0f64;
        let out: Vec<CvdBucketOut> = buckets
            .iter()
            .map(|b| {
                let delta = b.buy_qty - b.sell_qty;
                cvd_run += delta;
                CvdBucketOut {
                    bucket_ts_ms: b.bucket_ts_ms,
                    buy_qty: b.buy_qty,
                    sell_qty: b.sell_qty,
                    trades: b.trades,
                    delta,
                    cvd: cvd_run,
                }
            })
            .collect();
        let key = format!("binance_cvd_{}", sym.to_lowercase());
        let response = json!({
            "buckets": out,
            "bucket_secs": bucket_secs,
            "window_ms": window_ms,
            "count": out.len(),
            "computed_at_ms": now_ms,
        });
        let request = json!({"stream": format!("{}@aggTrade", sym.to_lowercase())});
        upsert_data_snapshot(pool, &key, &request, Some(&response), None, None).await?;
    }
    debug!(symbols = guard.len(), "cvd flushed");
    Ok(())
}

pub async fn aggtrade_cvd_loop(pool: PgPool) {
    info!("binance_aggtrade_cvd loop spawned");
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "binance_ws",
            "aggtrade_cvd.enabled",
            "QTSS_BINANCE_AGGTRADE_WS_ENABLED",
            true,
        )
        .await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        let flush_secs = resolve_worker_tick_secs(
            &pool,
            "binance_ws",
            "aggtrade_cvd.flush_secs",
            "QTSS_BINANCE_AGGTRADE_FLUSH_SECS",
            30,
            5,
        )
        .await;
        let window_secs = resolve_worker_tick_secs(
            &pool,
            "binance_ws",
            "aggtrade_cvd.window_secs",
            "QTSS_BINANCE_AGGTRADE_WINDOW_SECS",
            3600,
            300,
        )
        .await;
        let bucket_secs = resolve_worker_tick_secs(
            &pool,
            "binance_ws",
            "aggtrade_cvd.bucket_secs",
            "QTSS_BINANCE_AGGTRADE_BUCKET_SECS",
            60,
            5,
        )
        .await;
        let max_symbols = resolve_worker_tick_secs(
            &pool,
            "binance_ws",
            "aggtrade_cvd.max_symbols",
            "QTSS_BINANCE_AGGTRADE_MAX_SYMBOLS",
            20,
            1,
        )
        .await as usize;

        let symbols = match list_enabled_engine_symbols(&pool).await {
            Ok(v) => v,
            Err(e) => {
                warn!(%e, "aggtrade_cvd engine_symbols");
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        };
        let mut pairs = active_futures_pairs_lower(&symbols);
        if pairs.len() > max_symbols {
            pairs.truncate(max_symbols);
        }
        if pairs.is_empty() {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        let url = combined_url(&pairs, "aggTrade");
        info!(
            streams = pairs.len(),
            %flush_secs, %window_secs, %bucket_secs,
            "aggtrade_cvd connecting"
        );

        let buf: CvdBuffer = Arc::new(Mutex::new(HashMap::new()));
        let flush_buf = buf.clone();
        let flush_pool = pool.clone();
        let flush_window_ms = (window_secs as i64) * 1000;
        let flush_bucket = bucket_secs as i64;
        let flusher = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(flush_secs));
            tick.tick().await;
            loop {
                tick.tick().await;
                if let Err(e) =
                    flush_cvd(&flush_pool, &flush_buf, flush_window_ms, flush_bucket).await
                {
                    warn!(%e, "aggtrade_cvd flush");
                }
            }
        });

        match connect_url(&url).await {
            Ok(mut ws) => {
                info!("aggtrade_cvd ws connected");
                while let Some(msg) = ws.next().await {
                    match msg {
                        Ok(Message::Text(t)) => {
                            ingest_aggtrade(&t, &buf, bucket_secs as i64).await;
                        }
                        Ok(Message::Ping(p)) => {
                            let _ = ws.send(Message::Pong(p)).await;
                        }
                        Ok(Message::Close(_)) => break,
                        Err(e) => {
                            warn!(%e, "aggtrade_cvd ws read");
                            break;
                        }
                        _ => {}
                    }
                }
                warn!("aggtrade_cvd ws closed");
            }
            Err(e) => warn!(%e, "aggtrade_cvd connect"),
        }
        flusher.abort();
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_liquidation_basic() {
        let t = r#"{"stream":"btcusdt@forceOrder","data":{"e":"forceOrder","E":1,"o":{"s":"BTCUSDT","S":"SELL","o":"LIMIT","f":"IOC","q":"0.5","p":"40000","ap":"39995","X":"FILLED","T":1700000000000}}}"#;
        let (sym, ev) = parse_liquidation(t).unwrap();
        assert_eq!(sym, "BTCUSDT");
        assert_eq!(ev.side, "SELL");
        assert!((ev.qty - 0.5).abs() < 1e-9);
        assert_eq!(ev.ts_ms, 1_700_000_000_000);
    }

    #[tokio::test]
    async fn aggtrade_cvd_buckets() {
        let buf: CvdBuffer = Arc::new(Mutex::new(HashMap::new()));
        let a = r#"{"data":{"s":"BTCUSDT","q":"1.0","T":60000,"m":false}}"#;
        let b = r#"{"data":{"s":"BTCUSDT","q":"0.4","T":61000,"m":true}}"#;
        let c = r#"{"data":{"s":"BTCUSDT","q":"2.0","T":125000,"m":false}}"#;
        ingest_aggtrade(a, &buf, 60).await;
        ingest_aggtrade(b, &buf, 60).await;
        ingest_aggtrade(c, &buf, 60).await;
        let g = buf.lock().await;
        let v = g.get("BTCUSDT").unwrap();
        assert_eq!(v.len(), 2);
        assert!((v[0].buy_qty - 1.0).abs() < 1e-9);
        assert!((v[0].sell_qty - 0.4).abs() < 1e-9);
        assert!((v[1].buy_qty - 2.0).abs() < 1e-9);
    }

    #[test]
    fn combined_url_fmt() {
        let u = combined_url(&["btcusdt".into(), "ethusdt".into()], "forceOrder");
        assert!(u.contains("btcusdt@forceOrder"));
        assert!(u.contains("ethusdt@forceOrder"));
        assert!(u.starts_with("wss://fstream.binance.com/stream?streams="));
    }
}
