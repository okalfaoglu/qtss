//! Arka plan işleri: rollup, mutabakat; isteğe bağlı kline WebSocket → `market_bars`;
//! `engine_symbols` → analiz snapshot (Trading Range, …).

mod ai_engine;
mod ai_inference;
mod llm_judge;
mod detection_stats_refresh;
mod binance_catalog_sync_loop;
mod binance_futures_reconcile;
mod binance_spot_reconcile;
mod binance_public_ws;
mod binance_user_stream;
mod commission_sync_loop;
mod confluence;
mod confluence_hook;
mod dry_exchange_order;
mod copy_trade_follower;
mod copy_trade_queue;
mod data_sources;
mod engines;
mod feature_sources;
mod feature_store;
mod kill_switch;
// Faz 9.7.8 — live_position_notify removed.
mod nansen_credit_monitor;
mod nansen_engine;
mod nansen_extended;
mod nansen_query;
mod notify_outbox;
mod onchain_signal_scorer;
mod outcome_labeler_loop;
// Faz 9.7.8 — paper_fill_notify removed.
mod position_manager;
mod price_tick_ws;
mod setup_watcher;
mod x_publisher;
mod digest_loop;
mod selector_loop;
mod execution_bridge;
mod trainer_cron;
mod tick_dispatcher_loop;
mod setup_publisher;
mod range_signal_execute_loop;
mod setup_scan_engine;
mod signal_scorer;
mod strategy_runner;
mod ai_tactical_executor;
mod worker_probe_http;
mod engine_ingest;
mod intake_auto_promote;
mod intake_playbook_engine;
mod lifecycle_manager;
#[allow(dead_code, unused_variables, unused_imports)]
mod nansen_symbol_lifecycle;
// Faz 9.7.8 — position_status_notify removed.
mod v2_detection_orchestrator;
mod v2_detection_sweeper;
mod v2_projection_loop;
mod v2_detection_validator;
mod v2_pattern_strategy_bridge;
mod v2_risk_bridge;
mod v2_drawdown_snapshot;
mod v2_tbm_detector;
mod v2_onchain_loop;
mod v2_onchain_bridge;
mod v2_confluence_loop;
// Faz 9.7.8 — setup_chart removed (consumer v2_setup_telegram_loop gone).
mod v2_setup_loop;
// Faz 9.7.8 — v2_setup_telegram_loop removed.
mod wyckoff_setup_loop;
mod wyckoff_setup_invalidation_loop;
mod regime_deep_loop;
mod pivot_historical_backfill;
mod historical_progressive_scan;
mod data_health_report;

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
// `SinkExt`: required for WebSocket sink `.send` (trait methods are not inherent on the type).
use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use qtss_binance::{
    connect_url, kline_stream_path, parse_closed_kline_json, public_spot_combined_kline_url,
    public_spot_combined_streams_url, public_spot_kline_url, public_usdm_combined_kline_url,
    public_usdm_combined_streams_url, public_usdm_kline_url,
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
        let ingest_pool = pool.clone();
        tokio::spawn(engine_ingest::engine_symbol_ingest_loop(ingest_pool));
        let intake_pool = pool.clone();
        tokio::spawn(intake_playbook_engine::intake_playbook_loop(intake_pool));
        let auto_promote_pool = pool.clone();
        tokio::spawn(intake_auto_promote::intake_auto_promote_loop(auto_promote_pool));
        let lifecycle_pool = pool.clone();
        tokio::spawn(lifecycle_manager::lifecycle_manager_loop(lifecycle_pool));
        let nansen_lc_pool = pool.clone();
        tokio::spawn(nansen_symbol_lifecycle::nansen_symbol_lifecycle_loop(nansen_lc_pool));
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
        let nansen_tf = pool.clone();
        tokio::spawn(nansen_engine::nansen_tgm_flows_loop(nansen_tf));
        let nansen_tpt = pool.clone();
        tokio::spawn(nansen_engine::nansen_tgm_perp_trades_tgm_loop(nansen_tpt));
        let nansen_tdx = pool.clone();
        tokio::spawn(nansen_engine::nansen_tgm_dex_trades_loop(nansen_tdx));
        let nansen_tti = pool.clone();
        tokio::spawn(nansen_engine::nansen_tgm_token_information_loop(nansen_tti));
        let nansen_tind = pool.clone();
        tokio::spawn(nansen_engine::nansen_tgm_indicators_loop(nansen_tind));
        let nansen_tpp = pool.clone();
        tokio::spawn(nansen_engine::nansen_tgm_perp_positions_loop(nansen_tpp));
        let nansen_th = pool.clone();
        tokio::spawn(nansen_engine::nansen_tgm_holders_loop(nansen_th));
        let nansen_ps = pool.clone();
        tokio::spawn(nansen_engine::nansen_perp_screener_loop(nansen_ps));
        let nansen_smd = pool.clone();
        tokio::spawn(nansen_engine::nansen_smart_money_dex_trades_loop(nansen_smd));
        let nansen_cm = pool.clone();
        tokio::spawn(nansen_credit_monitor::nansen_credit_monitor_loop(nansen_cm));
        let setup_pool = pool.clone();
        tokio::spawn(setup_scan_engine::nansen_setup_scan_loop(setup_pool));
        // Auto-sync: aktif engine_symbols için eksik Binance veri kaynaklarını oluştur
        match qtss_storage::ensure_binance_sources_for_active_symbols(&pool).await {
            Ok(n) if n > 0 => tracing::info!(created = n, "Binance veri kaynakları otomatik oluşturuldu"),
            Err(e) => tracing::warn!(%e, "Binance auto-sync hatası"),
            _ => {}
        }
        let b_pool = pool.clone();
        tokio::spawn(engines::external_binance_loop(b_pool));
        // Faz 9.0.0 — Binance public WS feature streams (liquidations, CVD).
        let liq_pool = pool.clone();
        tokio::spawn(binance_public_ws::liquidation_stream_loop(liq_pool));
        let cvd_pool = pool.clone();
        tokio::spawn(binance_public_ws::aggtrade_cvd_loop(cvd_pool));
        // Faz 9.6 — Binance orderbook depth stream.
        let depth_pool = pool.clone();
        tokio::spawn(binance_public_ws::depth_stream_loop(depth_pool));
        // Faz 9.7.2 — Binance bookTicker stream → shared PriceTickStore.
        // Store is kept alive for the lifetime of the worker; the setup
        // watcher (9.7.3) will clone this Arc for read access.
        let price_store = qtss_notify::PriceTickStore::new();
        let tick_pool = pool.clone();
        tokio::spawn(price_tick_ws::price_tick_ws_loop(tick_pool, price_store.clone()));
        // Faz 9.7.3 — setup watcher (lifecycle dispatch + health).
        let watcher_pool = pool.clone();
        tokio::spawn(setup_watcher::setup_watcher_loop(watcher_pool, price_store.clone()));
        // Faz 9.7.6 — x_outbox publisher.
        let xpub_pool = pool.clone();
        tokio::spawn(x_publisher::x_publisher_loop(xpub_pool));
        // Faz 9.7.7 — per-user daily digest.
        let digest_pool = pool.clone();
        tokio::spawn(digest_loop::digest_loop(digest_pool));
        // Faz 9.7.8 — new-setup public broadcast (Telegram + x_outbox).
        let setup_pub_pool = pool.clone();
        tokio::spawn(setup_publisher::setup_publisher_loop(setup_pub_pool));
        // Faz 9.0.1 — outcome labeler (AI training substrate).
        let ol_pool = pool.clone();
        tokio::spawn(outcome_labeler_loop::outcome_labeler_loop(ol_pool));
        // Faz 9.8.11 — selector + execution bridge (setup → selected_candidates → placed).
        let sel_pool = pool.clone();
        tokio::spawn(selector_loop::selector_loop(sel_pool));
        // Faz 9.8.14/15 — shared LivePositionStore Arc so the tick
        // dispatcher and the execution_bridge see the same in-memory
        // state (fresh dry fills upsert → dispatcher picks them up on
        // the next sweep without waiting for 60s re-hydrate).
        let lp_store = std::sync::Arc::new(qtss_risk::LivePositionStore::new());
        let exb_pool = pool.clone();
        tokio::spawn(execution_bridge::execution_bridge_loop(
            exb_pool,
            lp_store.clone(),
        ));
        // Faz 9.8.12 — weekly trainer cron + AI sidecar health probe.
        let tr_pool = pool.clone();
        tokio::spawn(trainer_cron::trainer_cron_loop(tr_pool));
        // Faz 9.8.14 — tick dispatcher: hydrates LivePositionStore from DB,
        // polls PriceTickStore, runs evaluate_tick, persists outcomes.
        let td_pool = pool.clone();
        tokio::spawn(tick_dispatcher_loop::tick_dispatcher_loop(
            td_pool,
            lp_store.clone(),
            price_store.clone(),
        ));
        let cg_pool = pool.clone();
        tokio::spawn(engines::external_coinglass_loop(cg_pool));
        let hl_pool = pool.clone();
        tokio::spawn(engines::external_hyperliquid_loop(hl_pool));
        let misc_pool = pool.clone();
        tokio::spawn(engines::external_misc_loop(misc_pool));
        let onchain_pool = pool.clone();
        tokio::spawn(onchain_signal_scorer::onchain_signal_loop(onchain_pool));
        // Faz 9.7.8 — legacy notify loops removed.
        // `paper_fill_notify`, `live_position_notify`,
        // `position_status_notify`, and `v2_setup_telegram_loop` have
        // all been superseded by the Faz 9.7.x pipeline:
        //   * price_tick_ws  → shared PriceTickStore
        //   * setup_watcher  → lifecycle router (DbPersist + Telegram + XOutbox)
        //   * x_publisher    → drains x_outbox
        //   * digest_loop    → per-user daily digest
        // `notify_outbox_loop` stays — it still powers non-setup
        // notifications (auth, audit, etc.).
        let outbox_pool = pool.clone();
        tokio::spawn(notify_outbox::notify_outbox_loop(outbox_pool));
        let ks_pool = pool.clone();
        tokio::spawn(kill_switch::kill_switch_loop(ks_pool));
        let pm_pool = pool.clone();
        tokio::spawn(position_manager::position_manager_loop(pm_pool));
        let ct_pool = pool.clone();
        tokio::spawn(copy_trade_follower::copy_trade_follower_loop(ct_pool));
        let ctq_pool = pool.clone();
        tokio::spawn(copy_trade_queue::copy_trade_queue_loop(ctq_pool));
        let v2_det_pool = pool.clone();
        tokio::spawn(v2_detection_orchestrator::v2_detection_orchestrator_loop(
            v2_det_pool,
        ));
        let v2_sweep_pool = pool.clone();
        tokio::spawn(v2_detection_sweeper::v2_detection_sweeper_loop(v2_sweep_pool));
        let v2_tbm_pool = pool.clone();
        // Faz 7.7: TBM consumes onchain via the StoredV2OnchainProvider
        // bridge, which reads `qtss_v2_onchain_metrics` (populated by the
        // v2_onchain_loop fetcher pipeline). Stale-after window is fixed
        // to 30 minutes here; tunable via `onchain.stale_after_s` would
        // be a follow-up if operators ask for it.
        // P29c — caller-TF aware onchain bridge. ltf_cadence_s mirrors
        // the worker-side split in `onchain.aggregator.ltf_cadence_s`;
        // they stay in sync because both read from the same config key.
        let onchain_ltf_cadence_s = qtss_storage::resolve_system_u64(
            &v2_tbm_pool,
            "onchain",
            "aggregator.ltf_cadence_s",
            "QTSS_ONCHAIN_LTF_CAD",
            3600,
            60,
            86_400,
        )
        .await;
        let onchain_provider: std::sync::Arc<dyn qtss_tbm::onchain::OnchainMetricsProvider> =
            std::sync::Arc::new(v2_onchain_bridge::StoredV2OnchainProvider::with_ltf_cadence(
                v2_tbm_pool.clone(),
                1800,
                onchain_ltf_cadence_s,
            ));
        tokio::spawn(v2_tbm_detector::v2_tbm_detector_loop(
            v2_tbm_pool,
            onchain_provider,
        ));
        let v2_onchain_pool = pool.clone();
        tokio::spawn(v2_onchain_loop::v2_onchain_loop(v2_onchain_pool));
        let v2_conf_pool = pool.clone();
        tokio::spawn(v2_confluence_loop::v2_confluence_loop(v2_conf_pool));
        let v2_setup_pool = pool.clone();
        tokio::spawn(v2_setup_loop::v2_setup_loop(v2_setup_pool));
        let wyckoff_setup_pool = pool.clone();
        tokio::spawn(wyckoff_setup_loop::wyckoff_setup_loop(wyckoff_setup_pool));
        let wyckoff_inv_pool = pool.clone();
        tokio::spawn(wyckoff_setup_invalidation_loop::wyckoff_setup_invalidation_loop(
            wyckoff_inv_pool,
        ));
        let commission_sync_pool = pool.clone();
        tokio::spawn(commission_sync_loop::commission_sync_loop(
            commission_sync_pool,
        ));
        // Faz 9.7.8 — v2_setup_telegram_loop removed (replaced by
        // TelegramLifecycleHandler + XOutboxHandler on the setup watcher).
        let v2_val_pool = pool.clone();
        // Shared in-process event bus: the validator publishes
        // PATTERN_VALIDATED here, strategy providers subscribe.
        let v2_bus = std::sync::Arc::new(qtss_eventbus::InProcessBus::new());
        // Mirror the SSE-exported topics to Postgres NOTIFY so the
        // qtss-api SSE bridge can fan them out to browsers across the
        // process boundary. The handles are intentionally leaked: the
        // tasks should run for the entire worker lifetime.
        let _sse_export_handles = qtss_eventbus::PgNotifyExporter::start(
            v2_bus.clone(),
            pool.clone(),
            qtss_eventbus::topics::SSE_EXPORTED_TOPICS,
        );
        let v2_val_bus = v2_bus.clone();
        tokio::spawn(v2_detection_validator::v2_detection_validator_loop(
            v2_val_pool,
            v2_val_bus,
        ));
        // Keep the hit-rate outcome aggregate MV fresh (migration 0107).
        tokio::spawn(detection_stats_refresh::detection_stats_refresh_loop(pool.clone()));
        let v2_strat_pool = pool.clone();
        let v2_strat_bus = v2_bus.clone();
        tokio::spawn(v2_pattern_strategy_bridge::v2_pattern_strategy_bridge_loop(
            v2_strat_pool,
            v2_strat_bus,
        ));
        let v2_risk_pool = pool.clone();
        let v2_risk_bus = v2_bus.clone();
        tokio::spawn(v2_risk_bridge::v2_risk_bridge_loop(
            v2_risk_pool,
            v2_risk_bus,
        ));
        tokio::spawn(v2_drawdown_snapshot::v2_drawdown_snapshot_loop(pool.clone()));
        strategy_runner::spawn_if_enabled(&pool).await;
        ai_engine::spawn_ai_background_tasks(&pool).await;
        let ai_exec_pool = pool.clone();
        tokio::spawn(ai_tactical_executor::ai_tactical_executor_loop(ai_exec_pool));
        let regime_pool = pool.clone();
        tokio::spawn(regime_deep_loop::regime_deep_loop(regime_pool));
        let pivot_bf_pool = pool.clone();
        tokio::spawn(pivot_historical_backfill::pivot_historical_backfill_loop(
            pivot_bf_pool,
        ));
        let hps_pool = pool.clone();
        tokio::spawn(
            historical_progressive_scan::historical_progressive_scan_loop(hps_pool),
        );
        let health_pool = pool.clone();
        tokio::spawn(data_health_report::data_health_report_loop(health_pool));
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

    let env_symbols: Vec<String> = match pool_opt.as_ref() {
        Some(pool) => resolve_system_csv(pool, "worker", "kline_symbols_csv", "QTSS_KLINE_SYMBOLS", "").await,
        None => std::env::var("QTSS_KLINE_SYMBOLS").unwrap_or_default().split(',').map(|s| s.trim().to_string()).collect(),
    }
    .into_iter()
    .map(|s| s.trim().to_uppercase())
    .filter(|s| !s.is_empty())
    .collect();

    let market_data_exchange = resolve_market_data_exchange_id(pool_opt.as_ref()).await;
    let market_data_exchange_label = market_data_exchange.to_string();

    // Binance combined URL length guard — split into parallel sockets.
    const KLINE_WS_STREAM_CHUNK: usize = 48;

    let mut kline_started = false;
    if market_data_exchange == ExchangeId::Binance {
        if let Some(pool) = pool_opt.as_ref() {
            match list_enabled_engine_symbols(pool).await {
                Ok(rows) => {
                    let binance_rows: Vec<_> = rows
                        .into_iter()
                        .filter(|r| r.exchange.trim().eq_ignore_ascii_case("binance"))
                        .collect();
                    if !binance_rows.is_empty() {
                        let mut spot_paths: HashSet<String> = HashSet::new();
                        let mut fut_paths: HashSet<String> = HashSet::new();
                        for r in &binance_rows {
                            let path = kline_stream_path(r.symbol.trim(), r.interval.trim());
                            if segment_ws_db(r.segment.trim()) == "futures" {
                                fut_paths.insert(path);
                            } else {
                                spot_paths.insert(path);
                            }
                        }
                        let n_spot = spot_paths.len();
                        let n_fut = fut_paths.len();
                        let ex = market_data_exchange_label.clone();
                        for chunk in spot_paths.into_iter().collect::<Vec<_>>().chunks(KLINE_WS_STREAM_CHUNK) {
                            let paths = chunk.to_vec();
                            let p = pool.clone();
                            let exc = ex.clone();
                            tokio::spawn(multi_kline_ws_streams_loop(paths, "spot", Some(p), exc));
                        }
                        for chunk in fut_paths.into_iter().collect::<Vec<_>>().chunks(KLINE_WS_STREAM_CHUNK) {
                            let paths = chunk.to_vec();
                            let p = pool.clone();
                            let exc = ex.clone();
                            tokio::spawn(multi_kline_ws_streams_loop(paths, "futures", Some(p), exc));
                        }
                        info!(
                            spot_streams = n_spot,
                            futures_streams = n_fut,
                            exchange = %market_data_exchange_label,
                            "kline WebSocket: engine_symbols (per symbol+interval; spot and futures split)"
                        );
                        kline_started = true;
                    }
                }
                Err(e) => warn!(%e, "kline: engine_symbols read failed (WS fallback may use env symbols)"),
            }
        }
    }

    if !kline_started {
        let mut symbols = env_symbols;
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
                                    "kline: single-interval fallback from first engine_symbols row"
                                );
                            }
                            info!(
                                count = symbols.len(),
                                "kline WebSocket symbols from engine_symbols (unique symbol only; set QTSS_KLINE_SYMBOLS to override)"
                            );
                        }
                    }
                    Err(e) => warn!(%e, "kline: could not read engine_symbols for symbol fallback"),
                }
            }
        }

        if !symbols.is_empty() {
            if market_data_exchange == ExchangeId::Binance {
                info!(
                    count = symbols.len(),
                    %interval,
                    %segment,
                    exchange = %market_data_exchange_label,
                    from_engine = kline_symbols_from_engine,
                    "kline combined WebSocket starting (single interval for all symbols)"
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
                    "kline WebSocket off: add enabled Binance rows to engine_symbols, or set QTSS_KLINE_SYMBOLS / QTSS_KLINE_SYMBOL / worker.kline_symbols_csv"
                );
            }
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
        "future" | "futures" | "usdt_futures" | "fapi" => "futures",
        _ => "spot",
    }
}

fn kline_url(symbol: &str, interval: &str, segment: &str) -> String {
    match segment {
        "future" | "futures" | "usdt_futures" | "fapi" => public_usdm_kline_url(symbol, interval),
        _ => public_spot_kline_url(symbol, interval),
    }
}

fn combined_kline_url(symbols: &[String], interval: &str, segment: &str) -> String {
    match segment {
        "future" | "futures" | "usdt_futures" | "fapi" => public_usdm_combined_kline_url(symbols, interval),
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

/// Combined multiplex: each path is `symbol@kline_{interval}` (mixed intervals allowed).
async fn multi_kline_ws_streams_loop(
    stream_paths: Vec<String>,
    segment_db_key: &'static str,
    pool: Option<PgPool>,
    exchange: String,
) {
    if stream_paths.is_empty() {
        return;
    }
    let url = match segment_db_key {
        "futures" => public_usdm_combined_streams_url(&stream_paths),
        _ => public_spot_combined_streams_url(&stream_paths),
    };
    let seg_db = segment_db_key;
    info!(%url, streams = stream_paths.len(), %seg_db, "combined kline WebSocket (multi-interval)");
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
                                        tracing::debug!(symbol = %k.symbol, interval = %k.interval, "mum yazıldı");
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
