//! Arka plan işleri: rollup, mutabakat; isteğe bağlı kline WebSocket → `market_bars`;
//! `engine_symbols` → analiz snapshot (Trading Range, …).

mod ai_engine;
mod binance_catalog_sync_loop;
mod binance_futures_reconcile;
mod binance_spot_reconcile;
mod binance_user_stream;
mod confluence;
mod confluence_hook;
mod copy_trade_follower;
mod copy_trade_queue;
mod data_sources;
mod engines;
mod kill_switch;
mod live_position_notify;
mod nansen_engine;
mod nansen_extended;
mod nansen_query;
mod notify_outbox;
mod onchain_signal_scorer;
mod paper_fill_notify;
mod position_manager;
mod range_signal_execute_loop;
mod setup_scan_engine;
mod signal_scorer;
mod strategy_runner;
mod ai_tactical_executor;
mod worker_probe_http;

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
// `SinkExt`: required for WebSocket sink `.send` (trait methods are not inherent on the type).
use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use qtss_binance::{
    connect_url, parse_closed_kline_json, public_spot_combined_kline_url, public_spot_kline_url,
    public_usdm_combined_kline_url, public_usdm_kline_url,
};
use qtss_common::{init_logging, load_dotenv, postgres_url_from_env_or_default};
use qtss_domain::ExchangeId;
use qtss_storage::{
    create_pool, list_enabled_engine_symbols, resolve_worker_tick_secs, run_migrations,
    resolve_system_csv, resolve_system_string, upsert_market_bar, MarketBarUpsert,
    PnlRollupRepository,
};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{info, warn};

async fn pnl_rollup_loop(pool: PgPool) {
    let pnl = PnlRollupRepository::new(pool.clone());
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
        let sleep_secs = resolve_worker_tick_secs(
            &pool,
            "worker",
            "pnl_rollup_tick_secs",
            "QTSS_PNL_ROLLUP_TICK_SECS",
            300,
            60,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
    }
}

/// Primary venue for worker kline / `market_bars` ingestion (`QTSS_MASTER_DEV_GUIDE` §1.2 M4).
/// Only [`ExchangeId::Binance`] starts the embedded WebSocket loop; other values are reserved until adapters land.
async fn resolve_market_data_exchange_id(pool: Option<&PgPool>) -> ExchangeId {
    let raw = match pool {
        Some(p) => {
            resolve_system_string(
                p,
                "worker",
                "market_data_exchange",
                "QTSS_MARKET_DATA_EXCHANGE",
                "binance",
            )
            .await
        }
        None => std::env::var("QTSS_MARKET_DATA_EXCHANGE").unwrap_or_else(|_| "binance".into()),
    };
    let s = raw.trim().to_lowercase();
    match ExchangeId::from_str(&s) {
        Ok(id) => id,
        Err(_) => {
            warn!(
                value = %s,
                "QTSS_MARKET_DATA_EXCHANGE invalid, using binance",
            );
            ExchangeId::Binance
        }
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
        tracing::debug!(
            source_key = *k,
            "nansen HTTP snapshot key (nansen_extended)"
        );
    }

    let db_url = postgres_url_from_env_or_default("");
    let pool_opt: Option<PgPool> = if !db_url.trim().is_empty() {
        let pool = create_pool(&db_url, 3).await.context(
            "qtss-worker: PostgreSQL pool failed (check DATABASE_URL, host, port, credentials)",
        )?;
        run_migrations(&pool).await.context(
            "qtss-worker: SQL migrations failed — journalctl -u qtss-worker -n 100 --no-pager. \
             Yaygın: checksum uyuşmazlığı → `cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums` (DATABASE_URL); \
             `to_regclass('public.bar_intervals')` NULL → `0036_bar_intervals_repair_if_missing.sql` (API/worker migrate); \
             çift aynı `NNNN_*.sql` öneki. Ayrıntı: docs/QTSS_CURSOR_DEV_GUIDE.md §6.",
        )?;
        kill_switch::apply_initial_halt_from_db(&pool).await;
        let sync_pool = pool.clone();
        tokio::spawn(kill_switch::kill_switch_db_sync_loop(sync_pool));
        let pnl_pool = pool.clone();
        tokio::spawn(pnl_rollup_loop(pnl_pool));
        let catalog_pool = pool.clone();
        tokio::spawn(binance_catalog_sync_loop::binance_catalog_sync_loop(catalog_pool));
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
        let range_exec_pool = pool.clone();
        tokio::spawn(range_signal_execute_loop::range_signal_execute_loop(
            range_exec_pool,
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
        tokio::spawn(paper_fill_notify::paper_position_notify_loop(
            paper_notify_pool,
        ));
        let outbox_pool = pool.clone();
        tokio::spawn(notify_outbox::notify_outbox_loop(outbox_pool));
        let live_notify_pool = pool.clone();
        tokio::spawn(live_position_notify::live_position_notify_loop(
            live_notify_pool,
        ));
        let ks_pool = pool.clone();
        tokio::spawn(kill_switch::kill_switch_loop(ks_pool));
        let pm_pool = pool.clone();
        tokio::spawn(position_manager::position_manager_loop(pm_pool));
        let ct_pool = pool.clone();
        tokio::spawn(copy_trade_follower::copy_trade_follower_loop(ct_pool));
        let ctq_pool = pool.clone();
        tokio::spawn(copy_trade_queue::copy_trade_queue_loop(ctq_pool));
        strategy_runner::spawn_if_enabled(&pool).await;
        ai_engine::spawn_ai_background_tasks(&pool).await;
        let ai_exec_pool = pool.clone();
        tokio::spawn(ai_tactical_executor::ai_tactical_executor_loop(ai_exec_pool));
        binance_user_stream::spawn_binance_user_stream_tasks(&pool).await;
        Some(pool)
    } else {
        warn!("DATABASE_URL yok — pnl_rollups / market_bars DB yazımı kapalı");
        None
    };

    let (mut interval, mut segment) = match pool_opt.as_ref() {
        Some(pool) => {
            let interval = resolve_system_string(pool, "worker", "kline_interval", "QTSS_KLINE_INTERVAL", "1m").await;
            let segment = resolve_system_string(pool, "worker", "kline_segment", "QTSS_KLINE_SEGMENT", "spot").await;
            (interval, segment)
        }
        None => (
            std::env::var("QTSS_KLINE_INTERVAL").unwrap_or_else(|_| "1m".into()),
            std::env::var("QTSS_KLINE_SEGMENT").unwrap_or_else(|_| "spot".into()),
        ),
    };

    let mut symbols: Vec<String> = match pool_opt.as_ref() {
        Some(pool) => resolve_system_csv(pool, "worker", "kline_symbols_csv", "QTSS_KLINE_SYMBOLS", "").await,
        None => std::env::var("QTSS_KLINE_SYMBOLS").unwrap_or_default().split(',').map(|s| s.trim().to_string()).collect(),
    }
    .into_iter()
    .map(|s| s.trim().to_uppercase())
    .filter(|s| !s.is_empty())
    .collect();

    let mut kline_symbols_from_engine = false;
    if symbols.is_empty() {
        if let Some(pool) = pool_opt.as_ref() {
            match list_enabled_engine_symbols(pool).await {
                Ok(rows) => {
                    let mut seen = HashSet::new();
                    for r in &rows {
                        let s = r.symbol.trim().to_uppercase();
                        if !s.is_empty() && seen.insert(s.clone()) {
                            symbols.push(s);
                        }
                    }
                    if !symbols.is_empty() {
                        kline_symbols_from_engine = true;
                        if let Some(first) = rows.first() {
                            interval = first.interval.trim().to_string();
                            segment = first.segment.trim().to_string();
                            info!(
                                %interval,
                                %segment,
                                "kline: using interval/segment from first enabled engine_symbols row (override worker.kline_* / QTSS_KLINE_* when symbols fall back)"
                            );
                        }
                        info!(
                            count = symbols.len(),
                            "kline WebSocket symbols from enabled engine_symbols (set QTSS_KLINE_SYMBOLS or worker.kline_symbols_csv to override)"
                        );
                    }
                }
                Err(e) => warn!(%e, "kline: could not read engine_symbols for symbol fallback"),
            }
        }
    }

    let market_data_exchange = resolve_market_data_exchange_id(pool_opt.as_ref()).await;
    let market_data_exchange_label = market_data_exchange.to_string();

    if !symbols.is_empty() {
        if market_data_exchange == ExchangeId::Binance {
            info!(
                count = symbols.len(),
                %interval,
                %segment,
                exchange = %market_data_exchange_label,
                from_engine = kline_symbols_from_engine,
                "kline combined WebSocket starting"
            );
            let ex = market_data_exchange_label.clone();
            match pool_opt.as_ref() {
                Some(pool) => tokio::spawn(multi_kline_ws_loop(
                    symbols,
                    interval,
                    segment,
                    Some(pool.clone()),
                    ex,
                )),
                None => {
                    warn!("DATABASE_URL yok — combined kline yalnızca log");
                    tokio::spawn(multi_kline_ws_loop(symbols, interval, segment, None, ex))
                }
            };
        } else {
            warn!(
                count = symbols.len(),
                %interval,
                %segment,
                exchange = %market_data_exchange_label,
                "kline WebSocket skipped: multi-symbol feed is implemented for Binance only; set QTSS_MARKET_DATA_EXCHANGE=binance or worker.market_data_exchange",
            );
        }
    } else {
        let sym = match pool_opt.as_ref() {
            Some(pool) => {
                resolve_system_string(pool, "worker", "kline_symbol", "QTSS_KLINE_SYMBOL", "")
                    .await
            }
            None => std::env::var("QTSS_KLINE_SYMBOL").unwrap_or_default(),
        };
        let sym = sym.trim().to_string();
        if !sym.is_empty() {
            if market_data_exchange == ExchangeId::Binance {
                info!(
                    %sym,
                    %interval,
                    %segment,
                    exchange = %market_data_exchange_label,
                    "kline WebSocket starting (QTSS_KLINE_SYMBOL)",
                );
                let ex = market_data_exchange_label.clone();
                match pool_opt.as_ref() {
                    Some(pool) => tokio::spawn(kline_ws_loop(
                        sym,
                        interval,
                        segment,
                        Some(pool.clone()),
                        ex,
                    )),
                    None => {
                        warn!("DATABASE_URL yok — kline yalnızca log (market_bars yazılmaz)");
                        tokio::spawn(kline_ws_loop(sym, interval, segment, None, ex))
                    }
                };
            } else {
                warn!(
                    %sym,
                    %interval,
                    %segment,
                    exchange = %market_data_exchange_label,
                    "kline WebSocket skipped: feed is implemented for Binance only; set QTSS_MARKET_DATA_EXCHANGE=binance or worker.market_data_exchange",
                );
            }
        } else {
            warn!(
                "kline WebSocket off: set QTSS_KLINE_SYMBOLS or QTSS_KLINE_SYMBOL, worker.kline_symbols_csv, or enable engine_symbols rows. Example: QTSS_KLINE_SYMBOLS=BTCUSDT,ETHUSDT"
            );
        }
    }

    let bind = match pool_opt.as_ref() {
        Some(pool) => resolve_system_string(pool, "worker", "http_bind", "QTSS_WORKER_HTTP_BIND", "").await,
        None => std::env::var("QTSS_WORKER_HTTP_BIND").unwrap_or_default(),
    };
    if !bind.trim().is_empty() {
        let t = bind.trim();
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
        "futures" | "usdt_futures" | "fapi" => public_usdm_kline_url(symbol, interval),
        _ => public_spot_kline_url(symbol, interval),
    }
}

fn combined_kline_url(symbols: &[String], interval: &str, segment: &str) -> String {
    match segment {
        "futures" | "usdt_futures" | "fapi" => public_usdm_combined_kline_url(symbols, interval),
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
    exchange: String,
) {
    let url = combined_kline_url(&symbols, &interval, segment.as_str());
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
                                        persist_kline_closed_bar(pool, exchange.as_str(), seg_db, &k).await
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

async fn kline_ws_loop(
    symbol: String,
    interval: String,
    segment: String,
    pool: Option<PgPool>,
    exchange: String,
) {
    let url = kline_url(&symbol, &interval, segment.as_str());
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
                                    if let Err(e) =
                                        persist_kline_closed_bar(pool, exchange.as_str(), seg_db, &k).await
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
