//! Arka plan işleri: rollup, mutabakat; isteğe bağlı kline WebSocket → `market_bars`;
//! `engine_symbols` → analiz snapshot (Trading Range, …).

mod data_sources;
mod confluence_hook;
mod engines;
mod confluence;
mod nansen_engine;
mod nansen_extended;
mod signal_scorer;
mod nansen_query;
mod notify_outbox;
mod paper_fill_notify;
mod live_position_notify;
mod setup_scan_engine;
mod onchain_signal_scorer;
mod kill_switch;
mod position_manager;
mod copy_trade_follower;
mod copy_trade_queue;
mod binance_futures_reconcile;
mod binance_spot_reconcile;
mod strategy_runner;
mod worker_probe_http;

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use qtss_binance::{
    connect_url, parse_closed_kline_json, public_spot_combined_kline_url, public_spot_kline_url,
    public_usdm_combined_kline_url, public_usdm_kline_url,
};
use qtss_common::{ensure_postgres_scheme, init_logging, load_dotenv};
use anyhow::Context;
use qtss_storage::{
    create_pool, run_migrations, upsert_market_bar, MarketBarUpsert, PnlRollupRepository,
};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{info, warn};

async fn pnl_rollup_loop(pool: PgPool) {
    let pnl = PnlRollupRepository::new(pool);
    loop {
        match pnl.rebuild_live_rollups_from_exchange_orders().await {
            Ok(s) => info!(
                scanned = s.orders_scanned,
                fills = s.orders_with_fills,
                rows = s.rollup_rows_written,
                "pnl_rollups yenilendi"
            ),
            Err(e) => warn!(%e, "pnl_rollups rebuild"),
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv();
    // `sqlx::postgres::notice`: CREATE IF NOT EXISTS uyarıları (örn. _sqlx_migrations) INFO gürültüsünü keser.
    init_logging("info,qtss_worker=debug,sqlx::postgres::notice=warn");

    for r in crate::data_sources::registry::REGISTERED_DATA_SOURCES {
        tracing::debug!(
            source_key = r.source_key,
            provider_kind = r.provider_kind,
            description = r.description,
            "built-in data source registry entry"
        );
    }
    info!(
        count = crate::data_sources::registry::REGISTERED_DATA_SOURCES.len(),
        "worker: built-in data source registry (Phase G) — ayrıntı için qtss_worker=debug"
    );
    for k in crate::data_sources::registry::REGISTERED_NANSEN_HTTP_KEYS {
        tracing::debug!(source_key = *k, "nansen HTTP snapshot key (nansen_extended)");
    }

    let pool_opt: Option<PgPool> = match std::env::var("DATABASE_URL") {
        Ok(db_url) if !db_url.trim().is_empty() => {
            let db_url = db_url.trim().to_string();
            ensure_postgres_scheme(&db_url).context(
                "qtss-worker: DATABASE_URL postgres:// veya postgresql:// ile başlamalı (boş şema sqlx hatası verir)",
            )?;
            let pool = create_pool(&db_url, 3)
                .await
                .context("qtss-worker: PostgreSQL pool failed (check DATABASE_URL, host, port, credentials)")?;
            run_migrations(&pool).await.context(
                "qtss-worker: SQL migrations failed — journalctl -u qtss-worker -n 100 --no-pager. \
                 Yaygın: checksum uyuşmazlığı → `cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums` (DATABASE_URL); \
                 `to_regclass('public.bar_intervals')` NULL → `0036_bar_intervals_repair_if_missing.sql` (API/worker migrate); \
                 çift aynı `NNNN_*.sql` öneki. Ayrıntı: docs/QTSS_CURSOR_DEV_GUIDE.md §6.",
            )?;
            let pnl_pool = pool.clone();
            tokio::spawn(pnl_rollup_loop(pnl_pool));
            let reconcile_pool = pool.clone();
            tokio::spawn(binance_spot_reconcile::binance_spot_reconcile_loop(
                reconcile_pool,
            ));
            let reconcile_fut_pool = pool.clone();
            tokio::spawn(binance_futures_reconcile::binance_futures_reconcile_loop(
                reconcile_fut_pool,
            ));
            let engine_pool = pool.clone();
            let confluence_hook = Arc::new(confluence_hook::WorkerConfluenceHook);
            tokio::spawn(qtss_analysis::engine_analysis_loop(
                engine_pool,
                confluence_hook,
            ));
            let nansen_pool = pool.clone();
            tokio::spawn(nansen_engine::nansen_token_screener_loop(nansen_pool));
            let nansen_nf = pool.clone();
            tokio::spawn(nansen_engine::nansen_netflows_loop(nansen_nf));
            let nansen_h = pool.clone();
            tokio::spawn(nansen_engine::nansen_holdings_loop(nansen_h));
            let nansen_pt = pool.clone();
            tokio::spawn(nansen_engine::nansen_perp_trades_loop(nansen_pt));
            let nansen_wb = pool.clone();
            tokio::spawn(nansen_engine::nansen_who_bought_loop(nansen_wb));
            let nansen_fi = pool.clone();
            tokio::spawn(nansen_engine::nansen_flow_intel_loop(nansen_fi));
            let nansen_lb = pool.clone();
            tokio::spawn(nansen_engine::nansen_perp_leaderboard_loop(nansen_lb));
            let nansen_wh = pool.clone();
            tokio::spawn(nansen_engine::nansen_whale_perp_aggregate_loop(nansen_wh));
            let setup_pool = pool.clone();
            tokio::spawn(setup_scan_engine::nansen_setup_scan_loop(setup_pool));
            let b_pool = pool.clone();
            tokio::spawn(engines::external_binance_loop(b_pool));
            let cg_pool = pool.clone();
            tokio::spawn(engines::external_coinglass_loop(cg_pool));
            let hl_pool = pool.clone();
            tokio::spawn(engines::external_hyperliquid_loop(hl_pool));
            let misc_pool = pool.clone();
            tokio::spawn(engines::external_misc_loop(misc_pool));
            let onchain_pool = pool.clone();
            tokio::spawn(onchain_signal_scorer::onchain_signal_loop(onchain_pool));
            let paper_notify_pool = pool.clone();
            tokio::spawn(paper_fill_notify::paper_position_notify_loop(paper_notify_pool));
            let outbox_pool = pool.clone();
            tokio::spawn(notify_outbox::notify_outbox_loop(outbox_pool));
            let live_notify_pool = pool.clone();
            tokio::spawn(live_position_notify::live_position_notify_loop(live_notify_pool));
            let ks_pool = pool.clone();
            tokio::spawn(kill_switch::kill_switch_loop(ks_pool));
            let pm_pool = pool.clone();
            tokio::spawn(position_manager::position_manager_loop(pm_pool));
            let ct_pool = pool.clone();
            tokio::spawn(copy_trade_follower::copy_trade_follower_loop(ct_pool));
            let ctq_pool = pool.clone();
            tokio::spawn(copy_trade_queue::copy_trade_queue_loop(ctq_pool));
            strategy_runner::spawn_if_enabled(&pool);
            Some(pool)
        }
        _ => {
            warn!("DATABASE_URL yok — pnl_rollups / market_bars DB yazımı kapalı");
            None
        }
    };

    let interval = std::env::var("QTSS_KLINE_INTERVAL").unwrap_or_else(|_| "1m".into());
    let segment = std::env::var("QTSS_KLINE_SEGMENT").unwrap_or_else(|_| "spot".into());

    if let Ok(raw) = std::env::var("QTSS_KLINE_SYMBOLS") {
        let symbols: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty())
            .collect();
        if !symbols.is_empty() {
            info!(
                count = symbols.len(),
                %interval,
                %segment,
                "kline combined WebSocket başlatılıyor (QTSS_KLINE_SYMBOLS)"
            );
            match pool_opt.as_ref() {
                Some(pool) => {
                    tokio::spawn(multi_kline_ws_loop(
                        symbols,
                        interval,
                        segment,
                        Some(pool.clone()),
                    ));
                }
                None => {
                    warn!("DATABASE_URL yok — combined kline yalnızca log");
                    tokio::spawn(multi_kline_ws_loop(symbols, interval, segment, None));
                }
            }
        }
    } else if let Ok(sym) = std::env::var("QTSS_KLINE_SYMBOL") {
        let sym = sym.trim().to_string();
        if !sym.is_empty() {
            info!(%sym, %interval, %segment, "kline WebSocket görevi başlatılıyor (QTSS_KLINE_SYMBOL)");

            match pool_opt.as_ref() {
                Some(pool) => {
                    tokio::spawn(kline_ws_loop(sym, interval, segment, Some(pool.clone())));
                }
                None => {
                    warn!("DATABASE_URL yok — kline yalnızca log (market_bars yazılmaz)");
                    tokio::spawn(kline_ws_loop(sym, interval, segment, None));
                }
            }
        }
    } else {
        warn!(
            "QTSS_KLINE_SYMBOLS / QTSS_KLINE_SYMBOL tanımsız — kline WebSocket kapalı. \
             Örnek: QTSS_KLINE_SYMBOLS=BTCUSDT,ETHUSDT veya QTSS_KLINE_SYMBOL=BTCUSDT"
        );
    }

    if let Ok(raw) = std::env::var("QTSS_WORKER_HTTP_BIND") {
        let t = raw.trim();
        if !t.is_empty() {
            match t.parse::<std::net::SocketAddr>() {
                Ok(addr) => {
                    let probe_pool = pool_opt.clone();
                    tokio::spawn(async move {
                        if let Err(e) = worker_probe_http::serve(addr, probe_pool).await {
                            warn!(%e, "worker probe HTTP görevi sonlandı");
                        }
                    });
                }
                Err(e) => warn!(%e, bind = %t, "QTSS_WORKER_HTTP_BIND geçersiz, probe kapalı"),
            }
        }
    }

    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
        info!("worker heartbeat");
    }
}

fn segment_ws_db(segment: &str) -> &'static str {
    match segment {
        "futures" | "usdt_futures" | "fapi" => "futures",
        _ => "spot",
    }
}

fn kline_url(symbol: &str, interval: &str, segment: &str) -> String {
    match segment {
        "futures" | "usdt_futures" | "fapi" => {
            public_usdm_kline_url(symbol, interval)
        }
        _ => public_spot_kline_url(symbol, interval),
    }
}

fn combined_kline_url(symbols: &[String], interval: &str, segment: &str) -> String {
    match segment {
        "futures" | "usdt_futures" | "fapi" => {
            public_usdm_combined_kline_url(symbols, interval)
        }
        _ => public_spot_combined_kline_url(symbols, interval),
    }
}

fn decimal_field(s: &str, field: &'static str) -> Option<Decimal> {
    match Decimal::from_str(s.trim()) {
        Ok(d) => Some(d),
        Err(e) => {
            warn!(%e, %field, "geçersiz decimal");
            None
        }
    }
}

async fn persist_kline_closed_bar(
    pool: &PgPool,
    exchange: &str,
    seg_db: &str,
    k: &qtss_binance::ws_kline::ClosedKline,
) -> Result<(), qtss_storage::StorageError> {
    let Some(ot) = Utc.timestamp_millis_opt(k.open_time_ms).single() else {
        return Ok(());
    };
    let Some(open) = decimal_field(&k.open, "open") else {
        return Ok(());
    };
    let Some(high) = decimal_field(&k.high, "high") else {
        return Ok(());
    };
    let Some(low) = decimal_field(&k.low, "low") else {
        return Ok(());
    };
    let Some(close) = decimal_field(&k.close, "close") else {
        return Ok(());
    };
    let Some(volume) = decimal_field(&k.volume, "volume") else {
        return Ok(());
    };
    let quote_volume = k
        .quote_volume
        .as_deref()
        .and_then(|q| decimal_field(q, "quote_volume"));
    let trade_count = k.trade_count.map(|n| n as i64);
    let row = MarketBarUpsert {
        exchange: exchange.to_string(),
        segment: seg_db.to_string(),
        symbol: k.symbol.clone(),
        interval: k.interval.clone(),
        open_time: ot,
        open,
        high,
        low,
        close,
        volume,
        quote_volume,
        trade_count,
        instrument_id: None,
        bar_interval_id: None,
    };
    upsert_market_bar(pool, &row).await
}

async fn multi_kline_ws_loop(
    symbols: Vec<String>,
    interval: String,
    segment: String,
    pool: Option<PgPool>,
) {
    let url = combined_kline_url(&symbols, &interval, segment.as_str());
    let exchange = "binance";
    let seg_db = segment_ws_db(segment.as_str());
    info!(%url, "combined kline WebSocket");
    loop {
        match connect_url(&url).await {
            Ok(mut ws) => {
                info!(%url, "combined WebSocket bağlandı");
                while let Some(msg) = ws.next().await {
                    match msg {
                        Ok(Message::Text(t)) => {
                            if let Some(pool) = pool.as_ref() {
                                if let Some(k) = parse_closed_kline_json(&t) {
                                    if let Err(e) =
                                        persist_kline_closed_bar(pool, exchange, seg_db, &k).await
                                    {
                                        warn!(%e, symbol = %k.symbol, "market_bars upsert");
                                    } else {
                                        tracing::debug!(symbol = %k.symbol, "mum yazıldı");
                                    }
                                }
                            } else if t.len() < 400 {
                                tracing::debug!(%t, "kline combined");
                            }
                        }
                        Ok(Message::Ping(p)) => {
                            let _ = ws.send(Message::Pong(p)).await;
                        }
                        Ok(Message::Close(_)) => break,
                        Err(e) => {
                            warn!(%e, "ws okuma hatası");
                            break;
                        }
                        _ => {}
                    }
                }
                warn!("combined WebSocket kapandı, 5 sn sonra yeniden bağlanılacak");
            }
            Err(e) => {
                warn!(%e, "combined WebSocket bağlantı hatası");
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn kline_ws_loop(symbol: String, interval: String, segment: String, pool: Option<PgPool>) {
    let url = kline_url(&symbol, &interval, segment.as_str());
    let exchange = "binance";
    let seg_db = segment_ws_db(segment.as_str());

    loop {
        match connect_url(&url).await {
            Ok(mut ws) => {
                info!(%url, "WebSocket bağlandı");
                while let Some(msg) = ws.next().await {
                    match msg {
                        Ok(Message::Text(t)) => {
                            if let Some(pool) = pool.as_ref() {
                                if let Some(k) = parse_closed_kline_json(&t) {
                                    if let Err(e) = persist_kline_closed_bar(
                                        pool,
                                        exchange,
                                        seg_db,
                                        &k,
                                    )
                                    .await
                                    {
                                        warn!(%e, symbol = %k.symbol, "market_bars upsert");
                                    } else {
                                        info!(symbol = %k.symbol, interval = %k.interval, "mum yazıldı");
                                    }
                                }
                            } else if t.len() > 200 {
                                tracing::debug!(len = t.len(), "kline frame");
                            } else {
                                tracing::debug!(%t, "kline");
                            }
                        }
                        Ok(Message::Ping(p)) => {
                            let _ = ws.send(Message::Pong(p)).await;
                        }
                        Ok(Message::Close(_)) => break,
                        Err(e) => {
                            warn!(%e, "ws okuma hatası");
                            break;
                        }
                        _ => {}
                    }
                }
                warn!("WebSocket kapandı, 5 sn sonra yeniden bağlanılacak");
            }
            Err(e) => {
                warn!(%e, "WebSocket bağlantı hatası");
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
