//! Position summary + SL/TP monitoring (`QTSS_MASTER_DEV_GUIDE` FAZ 5.3).
//!
//! Derives net long exposure from `exchange_orders` fills; uses latest `market_bars` close as mark.
//! - `QTSS_POSITION_MANAGER_DRY_CLOSE_ENABLED=1` → [`DryRunGateway`] simulated exit.
//! - `QTSS_POSITION_MANAGER_LIVE_CLOSE_ENABLED=1` → Binance reduce-only market sell
//!   (`exchange_accounts` + `BinanceLiveGateway`); skipped when `is_trading_halted()`.
//!   Books whose `exchange_orders.exchange` is not Binance still get correct [`ExchangeId`] on
//!   dry intents. Live: **Binance** (spot/futures), **Bybit** linear, **OKX** USDT SWAP market
//!   reduce-only for AI directive close and SL/TP (`exchange_accounts.passphrase` required for OKX);
//!   managed / directive trailing remain Binance-only.
//!   If dry close is on, only the dry path runs (no conflict).
//!
//! Approved `ai_position_directives`: `activate_trailing` / tighten / widen (with optional
//! `trailing_callback_pct`), `deactivate_trailing` (clears managed trailing state), `partial_close`,
//! `full_close`; `add_to_position` is logged and not executed here.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use qtss_ai::feedback::record_decision_outcome;
use qtss_ai::storage::{
    fetch_latest_approved_directive, fetch_latest_approved_tactical, mark_applied, AiRecordTable,
};
use qtss_binance::{
    venue_order_id_from_binance_order_response, BinanceClient, BinanceClientConfig,
};
use qtss_common::is_trading_halted;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{FuturesExecutionExtras, OrderIntent, OrderSide, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::{
    venue_order_id_from_bybit_v5_response, venue_order_id_from_okx_v5_response, BinanceLiveGateway,
    BybitLiveGateway, DryRunGateway, OkxLiveGateway,
};
use qtss_storage::{
    list_recent_bars, resolve_system_decimal, resolve_system_string, resolve_worker_enabled_flag,
    resolve_worker_tick_secs, ExchangeAccountRepository, ExchangeOrderRepository, ExchangeOrderRow,
};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use serde_json::{json, Value as JsonValue};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Clone)]
struct PmLoopConfig {
    enabled: bool,
    dry_close: bool,
    live_close: bool,
    bar_interval: String,
    sl_pct: Decimal,
    tp_pct: Decimal,
    trailing_on_directive: bool,
    managed_trailing: bool,
    managed_cb_pct: Decimal,
    managed_limit_offset_pct: Decimal,
    managed_replace_step_pct: Decimal,
}

async fn load_pm_loop_config(pool: &PgPool) -> PmLoopConfig {
    let enabled = resolve_worker_enabled_flag(
        pool,
        "worker",
        "position_manager_enabled",
        "QTSS_POSITION_MANAGER_ENABLED",
        false,
    )
    .await;
    let dry_close = resolve_worker_enabled_flag(
        pool,
        "worker",
        "position_manager_dry_close_enabled",
        "QTSS_POSITION_MANAGER_DRY_CLOSE_ENABLED",
        false,
    )
    .await;
    let live_close = resolve_worker_enabled_flag(
        pool,
        "worker",
        "position_manager_live_close_enabled",
        "QTSS_POSITION_MANAGER_LIVE_CLOSE_ENABLED",
        false,
    )
    .await;
    let bar_interval = resolve_system_string(
        pool,
        "worker",
        "position_manager_bar_interval",
        "QTSS_POSITION_MANAGER_BAR_INTERVAL",
        "1m",
    )
    .await;
    let sl_pct = resolve_system_decimal(
        pool,
        "worker",
        "default_stop_loss_pct",
        "QTSS_DEFAULT_STOP_LOSS_PCT",
        Decimal::new(2, 0),
    )
    .await;
    let tp_pct = resolve_system_decimal(
        pool,
        "worker",
        "default_take_profit_pct",
        "QTSS_DEFAULT_TAKE_PROFIT_PCT",
        Decimal::new(4, 0),
    )
    .await;
    let trailing_on_directive = resolve_worker_enabled_flag(
        pool,
        "worker",
        "position_manager_trailing_on_directive",
        "QTSS_POSITION_MANAGER_TRAILING_ON_DIRECTIVE",
        false,
    )
    .await;
    let managed_trailing = resolve_worker_enabled_flag(
        pool,
        "worker",
        "position_manager_managed_trailing_enabled",
        "QTSS_POSITION_MANAGER_MANAGED_TRAILING_ENABLED",
        false,
    )
    .await;
    let managed_cb_pct = resolve_system_decimal(
        pool,
        "worker",
        "position_manager_managed_trailing_callback_rate_pct",
        "QTSS_POSITION_MANAGER_MANAGED_TRAILING_CALLBACK_RATE_PCT",
        Decimal::new(1, 0),
    )
    .await
        .max(Decimal::new(1, 1));
    let managed_limit_offset_pct = resolve_system_decimal(
        pool,
        "worker",
        "position_manager_managed_trailing_limit_offset_pct",
        "QTSS_POSITION_MANAGER_MANAGED_TRAILING_LIMIT_OFFSET_PCT",
        Decimal::new(2, 1),
    )
    .await
        .max(Decimal::new(1, 2));
    let managed_replace_step_pct = resolve_system_decimal(
        pool,
        "worker",
        "position_manager_managed_trailing_replace_step_pct",
        "QTSS_POSITION_MANAGER_MANAGED_TRAILING_REPLACE_STEP_PCT",
        Decimal::new(1, 1),
    )
    .await
        .max(Decimal::new(1, 2));
    PmLoopConfig {
        enabled,
        dry_close,
        live_close,
        bar_interval,
        sl_pct,
        tp_pct,
        trailing_on_directive,
        managed_trailing,
        managed_cb_pct,
        managed_limit_offset_pct,
        managed_replace_step_pct,
    }
}

#[derive(Clone, Default)]
struct ManagedTrailingState {
    peak: Decimal,
    active_cid: Option<Uuid>,
    active_stop: Decimal,
}

/// AI operational directive close request (immediate reduce-only exit).
#[derive(Clone, Copy)]
enum DirectiveCloseKind {
    Partial(Decimal),
    Full,
}

fn min_qty_filter() -> Decimal {
    Decimal::new(1, 8)
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct PosKey {
    user_id: Uuid,
    exchange: String,
    segment: String,
    symbol: String,
}

/// Parses `exchange_orders.exchange` (snake_case) for [`OrderIntent`] / live gating.
fn position_book_exchange_id(key: &PosKey) -> ExchangeId {
    let s = key.exchange.trim().to_lowercase();
    if s.is_empty() {
        return ExchangeId::Binance;
    }
    match ExchangeId::from_str(&s) {
        Ok(id) => id,
        Err(_) => {
            tracing::debug!(
                raw = %s,
                "position_book_exchange_id: unknown exchange label, using binance for intent"
            );
            ExchangeId::Binance
        }
    }
}

#[derive(Clone, Default)]
struct LongBook {
    qty: Decimal,
    cost: Decimal,
}

#[derive(Default)]
struct BookWithOrg {
    book: LongBook,
    org_id: Option<Uuid>,
}

fn market_reduce_long_intent(key: &PosKey, qty: Decimal) -> OrderIntent {
    let instrument = InstrumentId {
        exchange: position_book_exchange_id(key),
        segment: if key.segment.eq_ignore_ascii_case("futures") {
            MarketSegment::Futures
        } else {
            MarketSegment::Spot
        },
        symbol: key.symbol.clone(),
    };
    let futures = if key.segment.eq_ignore_ascii_case("futures") {
        Some(FuturesExecutionExtras {
            position_side: None,
            reduce_only: Some(true),
        })
    } else {
        None
    };
    OrderIntent {
        instrument,
        side: OrderSide::Sell,
        quantity: qty,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Gtc,
        requires_human_approval: false,
        futures,
    }
}

fn trailing_stop_reduce_long_intent(key: &PosKey, qty: Decimal, callback_rate_pct: Decimal) -> OrderIntent {
    let instrument = InstrumentId {
        exchange: position_book_exchange_id(key),
        segment: if key.segment.eq_ignore_ascii_case("futures") {
            MarketSegment::Futures
        } else {
            MarketSegment::Spot
        },
        symbol: key.symbol.clone(),
    };
    let futures = if key.segment.eq_ignore_ascii_case("futures") {
        Some(FuturesExecutionExtras {
            position_side: None,
            reduce_only: Some(true),
        })
    } else {
        None
    };
    OrderIntent {
        instrument,
        side: OrderSide::Sell,
        quantity: qty,
        order_type: OrderType::TrailingStopMarket {
            callback_rate: callback_rate_pct,
        },
        time_in_force: TimeInForce::Gtc,
        requires_human_approval: false,
        futures,
    }
}

fn parse_decimal_field(v: &JsonValue, k: &str) -> Option<Decimal> {
    v.get(k)
        .and_then(|x| x.as_str())
        .and_then(|s| Decimal::from_str(s.trim()).ok())
}

fn parse_executed_qty(venue: &JsonValue) -> Option<Decimal> {
    parse_decimal_field(venue, "executedQty")
}

fn parse_avg_price(venue: &JsonValue, qty: Decimal) -> Option<Decimal> {
    parse_decimal_field(venue, "avgPrice").or_else(|| {
        let qq = parse_decimal_field(venue, "cummulativeQuoteQty")?;
        if qty > Decimal::ZERO {
            Some(qq / qty)
        } else {
            None
        }
    })
}

fn intent_side(intent: &JsonValue) -> Option<OrderSide> {
    let s = intent.get("side")?.as_str()?.trim().to_ascii_lowercase();
    match s.as_str() {
        "buy" => Some(OrderSide::Buy),
        "sell" => Some(OrderSide::Sell),
        _ => None,
    }
}

fn update_long_book(book: &mut LongBook, side: OrderSide, qty: Decimal, price: Decimal) {
    match side {
        OrderSide::Buy => {
            book.cost += price * qty;
            book.qty += qty;
        }
        OrderSide::Sell => {
            let take = qty.min(book.qty);
            if take > Decimal::ZERO && book.qty > Decimal::ZERO {
                let avg = book.cost / book.qty;
                book.cost -= avg * take;
                book.qty -= take;
            }
        }
    }
    if book.qty <= Decimal::ZERO {
        book.qty = Decimal::ZERO;
        book.cost = Decimal::ZERO;
    }
}

fn aggregate_long_books(rows: &[ExchangeOrderRow]) -> HashMap<PosKey, BookWithOrg> {
    let mut sorted: Vec<_> = rows.iter().collect();
    sorted.sort_by_key(|r| r.created_at);
    let mut m: HashMap<PosKey, BookWithOrg> = HashMap::new();
    for row in sorted {
        let Some(venue) = row.venue_response.as_ref() else {
            continue;
        };
        let Some(qty) = parse_executed_qty(venue) else {
            continue;
        };
        if qty <= Decimal::ZERO {
            continue;
        };
        let Some(side) = intent_side(&row.intent) else {
            continue;
        };
        let Some(price) = parse_avg_price(venue, qty) else {
            continue;
        };
        let key = PosKey {
            user_id: row.user_id,
            exchange: row.exchange.trim().to_string(),
            segment: row.segment.trim().to_string(),
            symbol: row.symbol.trim().to_uppercase(),
        };
        let e = m.entry(key).or_default();
        if e.org_id.is_none() {
            e.org_id = Some(row.org_id);
        }
        update_long_book(&mut e.book, side, qty, price);
    }
    m
}

async fn last_close_price(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    interval: &str,
) -> Option<Decimal> {
    let bars = list_recent_bars(pool, exchange, segment, symbol, interval, 1)
        .await
        .ok()?;
    bars.into_iter().next().map(|b| b.close)
}

pub async fn position_manager_loop(pool: PgPool) {
    let pm_cfg = load_pm_loop_config(&pool).await;
    if !pm_cfg.enabled {
        info!("worker.position_manager_enabled kapalı — position_manager_loop çıkıyor");
        return;
    }
    let repo = ExchangeOrderRepository::new(pool.clone());
    let dry_gateway: Option<Arc<DryRunGateway>> = if pm_cfg.dry_close {
        Some(crate::strategy_runner::dry_gateway_from_pool(&pool).await)
    } else {
        None
    };
    let live_on = pm_cfg.live_close && dry_gateway.is_none();
    let bar_interval = pm_cfg.bar_interval.clone();
    info!(
        dry_close = dry_gateway.is_some(),
        live_close = live_on,
        "position_manager_loop: SL/TP izleme (poll: worker.position_manager_tick_secs / QTSS_POSITION_MANAGER_TICK_SECS)"
    );
    let sl = pm_cfg.sl_pct / Decimal::from(100u32);
    let tp = pm_cfg.tp_pct / Decimal::from(100u32);
    let min_q = min_qty_filter();
    let trailing_on_dir = pm_cfg.trailing_on_directive;
    let managed_trailing = pm_cfg.managed_trailing;
    let managed_cb_pct = pm_cfg.managed_cb_pct;
    let managed_limit_offset_pct = pm_cfg.managed_limit_offset_pct;
    let managed_replace_step_pct = pm_cfg.managed_replace_step_pct;
    let acct_repo = ExchangeAccountRepository::new(pool.clone());
    let live_gateway_cache: Mutex<HashMap<(Uuid, String), Arc<BinanceLiveGateway>>> =
        Mutex::new(HashMap::new());
    let bybit_live_gateway_cache: Mutex<HashMap<(Uuid, String), Arc<BybitLiveGateway>>> =
        Mutex::new(HashMap::new());
    let okx_live_gateway_cache: Mutex<HashMap<(Uuid, String), Arc<OkxLiveGateway>>> =
        Mutex::new(HashMap::new());
    let managed_trailing_state: Arc<Mutex<HashMap<PosKey, ManagedTrailingState>>> =
        Arc::new(Mutex::new(HashMap::new()));
    loop {
        let tick_secs = resolve_worker_tick_secs(
            &pool,
            "worker",
            "position_manager_tick_secs",
            "QTSS_POSITION_MANAGER_TICK_SECS",
            10,
            5,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
        let rows = match repo.list_recent_filled_orders_global(1500).await {
            Ok(r) => r,
            Err(e) => {
                warn!(%e, "position_manager list_recent_filled_orders_global");
                continue;
            }
        };
        let books = aggregate_long_books(&rows);
        for (key, agg) in books {
            let book_exchange_id = position_book_exchange_id(&key);
            let book = agg.book;
            if book.qty < min_q {
                continue;
            }
            let entry = if book.qty > Decimal::ZERO {
                book.cost / book.qty
            } else {
                continue;
            };
            let Some(mark) = last_close_price(
                &pool,
                &key.exchange,
                &key.segment,
                &key.symbol,
                bar_interval.trim(),
            )
            .await
            else {
                tracing::debug!(symbol = %key.symbol, "position_manager: bar yok");
                continue;
            };
            let mut sl_frac = sl;
            let mut tp_frac = tp;
            let mut tactical_applied_id: Option<Uuid> = None;
            let mut directive_applied_id: Option<Uuid> = None;
            let mut ai_outcome_decision_id: Option<Uuid> = None;
            let mut directive_requested_trailing = false;
            let mut directive_close: Option<DirectiveCloseKind> = None;
            let mut directive_trailing_callback_pct: Option<f64> = None;
            if let Ok(Some(td)) = fetch_latest_approved_tactical(&pool, &key.symbol).await {
                ai_outcome_decision_id = Some(td.decision_id);
                if let Some(p) = td.stop_loss_pct {
                    if let Some(d) = Decimal::from_f64(p) {
                        sl_frac = d / Decimal::from(100u32);
                    }
                }
                if let Some(p) = td.take_profit_pct {
                    if let Some(d) = Decimal::from_f64(p) {
                        tp_frac = d / Decimal::from(100u32);
                    }
                }
                tactical_applied_id = Some(td.id);
            }
            if let Ok(Some(dir)) = fetch_latest_approved_directive(&pool, &key.symbol).await {
                if ai_outcome_decision_id.is_none() {
                    ai_outcome_decision_id = Some(dir.decision_id);
                }
                if let Some(p) = dir.trailing_callback_pct.filter(|x| *x > 0.0) {
                    directive_trailing_callback_pct = Some(p);
                }
                match dir.action.as_str() {
                    "keep" => {}
                    "tighten_stop" => {
                        directive_requested_trailing = true;
                        if let Some(p) = dir.new_stop_loss_pct {
                            if let Some(d) = Decimal::from_f64(p) {
                                sl_frac = d / Decimal::from(100u32);
                            }
                        } else {
                            sl_frac = sl_frac * Decimal::new(9, 1) / Decimal::TEN;
                        }
                        if let Some(p) = dir.new_take_profit_pct {
                            if let Some(nt) = Decimal::from_f64(p) {
                                tp_frac = nt / Decimal::from(100u32);
                            }
                        }
                    }
                    "widen_stop" => {
                        directive_requested_trailing = true;
                        if let Some(p) = dir.new_stop_loss_pct {
                            if let Some(d) = Decimal::from_f64(p) {
                                sl_frac = d / Decimal::from(100u32);
                            }
                        } else {
                            sl_frac = sl_frac * Decimal::new(11, 1) / Decimal::TEN;
                        }
                        if let Some(p) = dir.new_take_profit_pct {
                            if let Some(nt) = Decimal::from_f64(p) {
                                tp_frac = nt / Decimal::from(100u32);
                            }
                        }
                    }
                    "activate_trailing" => {
                        directive_requested_trailing = true;
                        if let Some(p) = dir.new_stop_loss_pct {
                            if let Some(d) = Decimal::from_f64(p) {
                                sl_frac = d / Decimal::from(100u32);
                            }
                        }
                        if let Some(p) = dir.new_take_profit_pct {
                            if let Some(nt) = Decimal::from_f64(p) {
                                tp_frac = nt / Decimal::from(100u32);
                            }
                        }
                    }
                    "deactivate_trailing" => {
                        directive_requested_trailing = false;
                        managed_trailing_state
                            .lock()
                            .unwrap()
                            .remove(&key);
                    }
                    "partial_close" => {
                        let pct_raw = dir.partial_close_pct.unwrap_or(50.0).clamp(0.0, 100.0);
                        if pct_raw > 0.0 {
                            if let Some(ratio) = Decimal::from_f64(pct_raw / 100.0) {
                                let q = (book.qty * ratio).max(Decimal::ZERO);
                                if q >= min_q {
                                    directive_close = Some(DirectiveCloseKind::Partial(q));
                                }
                            }
                        }
                    }
                    "full_close" => {
                        directive_close = Some(DirectiveCloseKind::Full);
                    }
                    "add_to_position" => {
                        tracing::info!(
                            symbol = %key.symbol,
                            "position_manager: add_to_position directive ignored (not executed here)"
                        );
                    }
                    _ => {
                        tracing::warn!(
                            symbol = %key.symbol,
                            action = %dir.action,
                            "position_manager: unknown ai_position_directives.action"
                        );
                    }
                }
                directive_applied_id = Some(dir.id);
            }

            // AI directive: partial or full reduce-only close (before managed trailing / SL-TP path).
            if let Some(close_kind) = directive_close {
                let is_ai_full_close = matches!(close_kind, DirectiveCloseKind::Full);
                let qty_to_close = match close_kind {
                    DirectiveCloseKind::Partial(q) => q,
                    DirectiveCloseKind::Full => book.qty,
                };
                if qty_to_close < min_q {
                    if let Some(id) = directive_applied_id {
                        let _ = mark_applied(&pool, AiRecordTable::PositionDirectiveChild, id).await;
                    }
                    continue;
                }
                let intent = market_reduce_long_intent(&key, qty_to_close);
                let outcome_label = if mark > entry {
                    "profit"
                } else if (mark - entry).abs() < entry * Decimal::new(1, 4) {
                    "breakeven"
                } else {
                    "loss"
                };
                let outcome_db: &str = if outcome_label == "breakeven" {
                    "breakeven"
                } else {
                    outcome_label
                };
                let pnl_note = if is_ai_full_close {
                    "ai_directive_full_close"
                } else {
                    "ai_directive_partial_close"
                };
                let mut close_executed = false;
                if let Some(ref gw) = dry_gateway {
                    match crate::dry_exchange_order::persist_after_dry_place(
                        gw.as_ref(),
                        &pool,
                        &repo,
                        &intent,
                        mark,
                        &key.symbol,
                        key.exchange.trim(),
                        key.segment.trim(),
                        "position_manager_ai_directive_close",
                        json!({ "pnl_note": pnl_note }),
                        &rows,
                        agg.org_id,
                        Some(key.user_id),
                    )
                    .await
                    {
                        Ok(_) => {
                            close_executed = true;
                            if let Some(did) = ai_outcome_decision_id {
                                let pnl_pct = entry.to_f64().and_then(|e| {
                                    mark.to_f64()
                                        .and_then(|m| (e > 0.0).then_some((m - e) / e * 100.0))
                                });
                                if let Err(e) = record_decision_outcome(
                                    &pool,
                                    did,
                                    pnl_pct,
                                    None,
                                    outcome_db,
                                    None,
                                    Some(pnl_note),
                                )
                                .await
                                {
                                    warn!(%e, symbol = %key.symbol, "record_decision_outcome (directive dry)");
                                }
                            }
                        }
                        Err(e) => {
                            warn!(%e, symbol = %key.symbol, "position_manager: dry directive close place failed");
                        }
                    }
                } else if live_on {
                    if is_trading_halted() {
                        warn!(
                            user_id = %key.user_id,
                            symbol = %key.symbol,
                            "position_manager: directive close skipped — trading halted"
                        );
                    } else if let Some(org_id) = agg.org_id {
                        if book_exchange_id == ExchangeId::Binance {
                            let creds = match acct_repo
                                .binance_for_user(key.user_id, key.segment.trim())
                                .await
                            {
                                Ok(c) => c,
                                Err(e) => {
                                    warn!(%e, "position_manager: exchange_accounts (directive close)");
                                    None
                                }
                            };
                            if let Some(creds) = creds {
                                let seg_norm = key.segment.trim().to_lowercase();
                                let gw_cache_key = (key.user_id, seg_norm);
                                let gw = {
                                    let mut guard = live_gateway_cache.lock().unwrap();
                                    if let Some(g) = guard.get(&gw_cache_key) {
                                        g.clone()
                                    } else {
                                        let cfg = BinanceClientConfig::mainnet_with_keys(
                                            creds.api_key,
                                            creds.api_secret,
                                        );
                                        let client = match BinanceClient::new(cfg) {
                                            Ok(c) => Arc::new(c),
                                            Err(e) => {
                                                warn!(%e, "position_manager: BinanceClient (directive close)");
                                                continue;
                                            }
                                        };
                                        let g = Arc::new(BinanceLiveGateway::new(client));
                                        guard.insert(gw_cache_key, g.clone());
                                        g
                                    }
                                };
                                let intent_record = intent.clone();
                                if let Ok((cid, venue_json)) =
                                    gw.place_with_venue_response(intent).await
                                {
                                    let venue_oid =
                                        venue_order_id_from_binance_order_response(&venue_json);
                                    match repo
                                        .insert_submitted(
                                            org_id,
                                            key.user_id,
                                            key.exchange.trim(),
                                            key.segment.trim(),
                                            &key.symbol,
                                            cid,
                                            &intent_record,
                                            venue_oid,
                                            Some(venue_json),
                                        )
                                        .await
                                    {
                                        Ok(_) => {
                                            close_executed = true;
                                            if let Some(did) = ai_outcome_decision_id {
                                                let pnl_pct = entry.to_f64().and_then(|e| {
                                                    mark.to_f64().and_then(|m| {
                                                        (e > 0.0).then_some((m - e) / e * 100.0)
                                                    })
                                                });
                                                if let Err(e) = record_decision_outcome(
                                                    &pool,
                                                    did,
                                                    pnl_pct,
                                                    None,
                                                    outcome_db,
                                                    None,
                                                    Some(pnl_note),
                                                )
                                                .await
                                                {
                                                    warn!(%e, symbol = %key.symbol, "record_decision_outcome (directive live)");
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            warn!(%e, %cid, "position_manager: directive close DB insert failed");
                                        }
                                    }
                                } else {
                                    warn!(symbol = %key.symbol, "position_manager: live directive close place failed");
                                }
                            }
                        } else if book_exchange_id == ExchangeId::Bybit
                            && key.segment.eq_ignore_ascii_case("futures")
                        {
                            let creds = match acct_repo
                                .credentials_for_user(
                                    key.user_id,
                                    "bybit",
                                    key.segment.trim(),
                                )
                                .await
                            {
                                Ok(c) => c,
                                Err(e) => {
                                    warn!(%e, "position_manager: exchange_accounts bybit (directive close)");
                                    None
                                }
                            };
                            if let Some(creds) = creds {
                                let seg_norm = key.segment.trim().to_lowercase();
                                let gw_cache_key = (key.user_id, seg_norm.clone());
                                let gw = {
                                    let mut guard = bybit_live_gateway_cache.lock().unwrap();
                                    if let Some(g) = guard.get(&gw_cache_key) {
                                        g.clone()
                                    } else {
                                        let g = Arc::new(BybitLiveGateway::mainnet(
                                            creds.api_key,
                                            creds.api_secret,
                                        ));
                                        guard.insert(gw_cache_key, g.clone());
                                        g
                                    }
                                };
                                let intent_record = intent.clone();
                                if let Ok((cid, venue_json)) =
                                    gw.place_with_venue_response(intent).await
                                {
                                    let venue_oid =
                                        venue_order_id_from_bybit_v5_response(&venue_json);
                                    match repo
                                        .insert_submitted(
                                            org_id,
                                            key.user_id,
                                            key.exchange.trim(),
                                            key.segment.trim(),
                                            &key.symbol,
                                            cid,
                                            &intent_record,
                                            venue_oid,
                                            Some(venue_json),
                                        )
                                        .await
                                    {
                                        Ok(_) => {
                                            close_executed = true;
                                            if let Some(did) = ai_outcome_decision_id {
                                                let pnl_pct = entry.to_f64().and_then(|e| {
                                                    mark.to_f64().and_then(|m| {
                                                        (e > 0.0).then_some((m - e) / e * 100.0)
                                                    })
                                                });
                                                if let Err(e) = record_decision_outcome(
                                                    &pool,
                                                    did,
                                                    pnl_pct,
                                                    None,
                                                    outcome_db,
                                                    None,
                                                    Some(pnl_note),
                                                )
                                                .await
                                                {
                                                    warn!(%e, symbol = %key.symbol, "record_decision_outcome (directive live bybit)");
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            warn!(%e, %cid, "position_manager: directive close DB insert failed (bybit)");
                                        }
                                    }
                                } else {
                                    warn!(
                                        symbol = %key.symbol,
                                        "position_manager: live directive close place failed (bybit)"
                                    );
                                }
                            }
                        } else if book_exchange_id == ExchangeId::Okx
                            && key.segment.eq_ignore_ascii_case("futures")
                        {
                            let creds =
                                match acct_repo
                                    .credentials_for_user(
                                        key.user_id,
                                        "okx",
                                        key.segment.trim(),
                                    )
                                    .await
                                {
                                    Ok(c) => c,
                                    Err(e) => {
                                        warn!(%e, "position_manager: exchange_accounts okx (directive close)");
                                        None
                                    }
                                };
                            if let Some(creds) = creds {
                                let passphrase = creds.passphrase.clone().unwrap_or_default();
                                if passphrase.trim().is_empty() {
                                    warn!(
                                        user_id = %key.user_id,
                                        symbol = %key.symbol,
                                        "position_manager: OKX passphrase missing (exchange_accounts.passphrase)",
                                    );
                                } else {
                                    let seg_norm = key.segment.trim().to_lowercase();
                                    let gw_cache_key = (key.user_id, seg_norm.clone());
                                    let gw = {
                                        let mut guard = okx_live_gateway_cache.lock().unwrap();
                                        if let Some(g) = guard.get(&gw_cache_key) {
                                            g.clone()
                                        } else {
                                            let g = Arc::new(OkxLiveGateway::mainnet(
                                                creds.api_key,
                                                creds.api_secret,
                                                passphrase,
                                            ));
                                            guard.insert(gw_cache_key, g.clone());
                                            g
                                        }
                                    };
                                    let intent_record = intent.clone();
                                    if let Ok((cid, venue_json)) =
                                        gw.place_with_venue_response(intent).await
                                    {
                                        let venue_oid =
                                            venue_order_id_from_okx_v5_response(&venue_json);
                                        match repo
                                            .insert_submitted(
                                                org_id,
                                                key.user_id,
                                                key.exchange.trim(),
                                                key.segment.trim(),
                                                &key.symbol,
                                                cid,
                                                &intent_record,
                                                venue_oid,
                                                Some(venue_json),
                                            )
                                            .await
                                        {
                                            Ok(_) => {
                                                close_executed = true;
                                                if let Some(did) = ai_outcome_decision_id {
                                                    let pnl_pct = entry.to_f64().and_then(|e| {
                                                        mark.to_f64().and_then(|m| {
                                                            (e > 0.0).then_some((m - e) / e * 100.0)
                                                        })
                                                    });
                                                    if let Err(e) = record_decision_outcome(
                                                        &pool,
                                                        did,
                                                        pnl_pct,
                                                        None,
                                                        outcome_db,
                                                        None,
                                                        Some(pnl_note),
                                                    )
                                                    .await
                                                    {
                                                        warn!(%e, symbol = %key.symbol, "record_decision_outcome (directive live okx)");
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                warn!(%e, %cid, "position_manager: directive close DB insert failed (okx)");
                                            }
                                        }
                                    } else {
                                        warn!(
                                            symbol = %key.symbol,
                                            "position_manager: live directive close place failed (okx)"
                                        );
                                    }
                                }
                            }
                        } else {
                            warn!(
                                user_id = %key.user_id,
                                symbol = %key.symbol,
                                exchange = %key.exchange,
                                ?book_exchange_id,
                                "position_manager: live directive close skipped — Binance, Bybit, or OKX USDT futures only",
                            );
                        }
                    }
                }
                if close_executed {
                    if let Some(id) = directive_applied_id {
                        let _ = mark_applied(&pool, AiRecordTable::PositionDirectiveChild, id).await;
                    }
                }
                continue;
            }

            // Managed trailing-stop-limit like behavior (futures only):
            // Keep a reduce-only STOP (limit) order whose stop moves up with peak price.
            if managed_trailing
                && live_on
                && book_exchange_id == ExchangeId::Binance
                && key.segment.eq_ignore_ascii_case("futures")
            {
                let (stop_price, should_replace, start_ok) = {
                    let mut st_guard = managed_trailing_state.lock().unwrap();
                    let state = st_guard.entry(key.clone()).or_default();
                    if state.peak <= Decimal::ZERO || mark > state.peak {
                        state.peak = mark;
                    }
                    let stop_price =
                        state.peak * (Decimal::ONE - managed_cb_pct / Decimal::from(100u32));
                    let should_replace = state.active_cid.is_none()
                        || (stop_price
                            > state.active_stop
                                * (Decimal::ONE
                                    + managed_replace_step_pct / Decimal::from(100u32)));
                    let start_ok = trailing_on_dir && directive_requested_trailing;
                    (stop_price, should_replace, start_ok)
                };
                if start_ok && should_replace {
                    if is_trading_halted() {
                        warn!(user_id=%key.user_id, symbol=%key.symbol, "managed trailing: skipped — trading halted");
                    } else if let Some(org_id) = agg.org_id {
                        let creds = acct_repo
                            .binance_for_user(key.user_id, key.segment.trim())
                            .await
                            .ok()
                            .flatten();
                        if let Some(creds) = creds {
                            let seg_norm = key.segment.trim().to_lowercase();
                            let gw_cache_key = (key.user_id, seg_norm);
                            let gw = {
                                let mut guard = live_gateway_cache.lock().unwrap();
                                if let Some(g) = guard.get(&gw_cache_key) {
                                    g.clone()
                                } else {
                                    let cfg = BinanceClientConfig::mainnet_with_keys(
                                        creds.api_key,
                                        creds.api_secret,
                                    );
                                    let client = match BinanceClient::new(cfg) {
                                        Ok(c) => Arc::new(c),
                                        Err(e) => {
                                            warn!(%e, "managed trailing: BinanceClient oluşturulamadı");
                                            continue;
                                        }
                                    };
                                    let g = Arc::new(BinanceLiveGateway::new(client));
                                    guard.insert(gw_cache_key, g.clone());
                                    g
                                }
                            };

                            // Open-order tracking: if we had an active_cid, verify it's still open; if open, cancel it.
                            let cur = {
                                let mut guard = managed_trailing_state.lock().unwrap();
                                guard.entry(key.clone()).or_default().clone()
                            };

                            if let Some(cid) = cur.active_cid {
                                match gw
                                    .futures_is_open_by_client_order_id(&key.symbol, &cid)
                                    .await
                                {
                                    Ok(true) => {
                                        // best-effort cancel; if cancel fails, re-check to avoid duplicate new order.
                                        if gw
                                            .cancel_futures_by_client_order_id(&key.symbol, &cid)
                                            .await
                                            .is_err()
                                        {
                                            if gw
                                                .futures_is_open_by_client_order_id(&key.symbol, &cid)
                                                .await
                                                .unwrap_or(false)
                                            {
                                                warn!(
                                                    symbol=%key.symbol,
                                                    %cid,
                                                    "managed trailing: cancel failed and order still open; skip replace"
                                                );
                                                continue;
                                            }
                                        }
                                    }
                                    Ok(false) => {
                                        // Not open anymore: clear local state to prevent noisy cancel/replace.
                                        let mut st_guard3 = managed_trailing_state.lock().unwrap();
                                        let e = st_guard3.entry(key.clone()).or_default();
                                        e.active_cid = None;
                                        e.active_stop = Decimal::ZERO;
                                    }
                                    Err(e) => {
                                        warn!(%e, symbol=%key.symbol, "managed trailing: openOrders check failed; skip replace");
                                        continue;
                                    }
                                }
                            }

                            let limit_price = stop_price
                                * (Decimal::ONE - managed_limit_offset_pct / Decimal::from(100u32));
                            let intent = OrderIntent {
                                instrument: InstrumentId {
                                    exchange: book_exchange_id,
                                    segment: MarketSegment::Futures,
                                    symbol: key.symbol.clone(),
                                },
                                side: OrderSide::Sell,
                                quantity: book.qty,
                                order_type: OrderType::StopLimit {
                                    stop_price,
                                    limit_price,
                                },
                                time_in_force: TimeInForce::Gtc,
                                requires_human_approval: false,
                                futures: Some(FuturesExecutionExtras {
                                    position_side: None,
                                    reduce_only: Some(true),
                                }),
                            };
                            let intent_record = intent.clone();
                            match gw.place_with_venue_response(intent).await {
                                Ok((cid, venue_json)) => {
                                    let venue_oid = venue_order_id_from_binance_order_response(&venue_json);
                                    let _ = repo
                                        .insert_submitted(
                                            org_id,
                                            key.user_id,
                                            key.exchange.trim(),
                                            key.segment.trim(),
                                            &key.symbol,
                                            cid,
                                            &intent_record,
                                            venue_oid,
                                            Some(venue_json),
                                        )
                                        .await;
                                    let mut st_guard3 = managed_trailing_state.lock().unwrap();
                                    let e = st_guard3.entry(key.clone()).or_default();
                                    e.active_cid = Some(cid);
                                    e.active_stop = stop_price;
                                    if mark > e.peak {
                                        e.peak = mark;
                                    }
                                    info!(
                                        %cid,
                                        symbol=%key.symbol,
                                        stop=%stop_price,
                                        limit=%limit_price,
                                        peak=%e.peak,
                                        "managed trailing: placed stop-limit"
                                    );
                                }
                                Err(e) => warn!(%e, symbol=%key.symbol, "managed trailing: place stop-limit failed"),
                            }
                        }
                    }
                }

                if let Some(id) = directive_applied_id {
                    let _ = mark_applied(&pool, AiRecordTable::PositionDirectiveChild, id).await;
                }
                // Managed trailing has priority over SL/TP close for futures.
                continue;
            }

            // AI directive integration for trailing stop (Binance futures):
            // if directive adjusts stop, place a reduce-only trailing-stop-market instead of waiting for SL/TP hit.
            if trailing_on_dir
                && directive_requested_trailing
                && live_on
                && book_exchange_id == ExchangeId::Binance
                && key.segment.eq_ignore_ascii_case("futures")
            {
                if is_trading_halted() {
                    warn!(
                        user_id = %key.user_id,
                        symbol = %key.symbol,
                        "position_manager: trailing stop skipped — trading halted"
                    );
                } else if let Some(org_id) = agg.org_id {
                    let creds = acct_repo
                        .binance_for_user(key.user_id, key.segment.trim())
                        .await
                        .ok()
                        .flatten();
                    if let Some(creds) = creds {
                        let seg_norm = key.segment.trim().to_lowercase();
                        let gw_cache_key = (key.user_id, seg_norm);
                        let gw = {
                            let mut guard = live_gateway_cache.lock().unwrap();
                            if let Some(g) = guard.get(&gw_cache_key) {
                                g.clone()
                            } else {
                                let cfg = BinanceClientConfig::mainnet_with_keys(
                                    creds.api_key,
                                    creds.api_secret,
                                );
                                let client = match BinanceClient::new(cfg) {
                                    Ok(c) => Arc::new(c),
                                    Err(e) => {
                                        warn!(%e, "position_manager: BinanceClient oluşturulamadı (trailing)");
                                        continue;
                                    }
                                };
                                let g = Arc::new(BinanceLiveGateway::new(client));
                                guard.insert(gw_cache_key, g.clone());
                                g
                            }
                        };

                        // Binance TRAILING_STOP_MARKET: callbackRate percent (AI `trailing_callback_pct` or SL-derived).
                        let cb_pct = directive_trailing_callback_pct
                            .and_then(Decimal::from_f64)
                            .filter(|d| *d >= Decimal::new(1, 1))
                            .unwrap_or_else(|| {
                                (sl_frac * Decimal::from(100u32)).max(Decimal::new(1, 1))
                            });
                        let intent = trailing_stop_reduce_long_intent(&key, book.qty, cb_pct);
                        let intent_record = intent.clone();
                        match gw.place_with_venue_response(intent).await {
                            Ok((cid, venue_json)) => {
                                let venue_oid = venue_order_id_from_binance_order_response(&venue_json);
                                let _ = repo
                                    .insert_submitted(
                                        org_id,
                                        key.user_id,
                                        key.exchange.trim(),
                                        key.segment.trim(),
                                        &key.symbol,
                                        cid,
                                        &intent_record,
                                        venue_oid,
                                        Some(venue_json),
                                    )
                                    .await;
                                info!(%cid, symbol = %key.symbol, callback_rate_pct=%cb_pct, "position_manager: trailing stop placed (directive)");
                            }
                            Err(e) => warn!(%e, symbol=%key.symbol, "position_manager: trailing stop place failed (directive)"),
                        }
                    }
                }

                if let Some(id) = directive_applied_id {
                    let _ = mark_applied(&pool, AiRecordTable::PositionDirectiveChild, id).await;
                }
                // Do not do SL/TP close this tick when a trailing stop was requested/placed.
                continue;
            }

            let sl_price = entry * (Decimal::ONE - sl_frac);
            let tp_price = entry * (Decimal::ONE + tp_frac);
            let hit_sl = mark <= sl_price;
            let hit_tp = mark >= tp_price;
            if tactical_applied_id.is_some() || directive_applied_id.is_some() {
                if let Some(id) = tactical_applied_id {
                    if let Err(e) = mark_applied(&pool, AiRecordTable::TacticalChild, id).await {
                        warn!(%e, symbol = %key.symbol, "mark tactical AI applied");
                    }
                }
                if let Some(id) = directive_applied_id {
                    if let Err(e) =
                        mark_applied(&pool, AiRecordTable::PositionDirectiveChild, id).await
                    {
                        warn!(%e, symbol = %key.symbol, "mark position directive applied");
                    }
                }
            }
            if !hit_sl && !hit_tp {
                continue;
            }
            warn!(
                user_id = %key.user_id,
                symbol = %key.symbol,
                segment = %key.segment,
                net_qty = %book.qty,
                entry = %entry,
                mark = %mark,
                hit_sl,
                hit_tp,
                "position_manager: SL/TP eşiği"
            );
            let intent = market_reduce_long_intent(&key, book.qty);
            if let Some(ref gw) = dry_gateway {
                match crate::dry_exchange_order::persist_after_dry_place(
                    gw.as_ref(),
                    &pool,
                    &repo,
                    &intent,
                    mark,
                    &key.symbol,
                    key.exchange.trim(),
                    key.segment.trim(),
                    "position_manager_sl_tp",
                    json!({ "hit_sl": hit_sl, "hit_tp": hit_tp }),
                    &rows,
                    agg.org_id,
                    Some(key.user_id),
                )
                .await
                {
                    Ok(out) => {
                        info!(
                            cid = %out.client_order_id,
                            symbol = %key.symbol,
                            "position_manager: dry kapatma emri (exchange_orders)"
                        );
                        if let Some(did) = ai_outcome_decision_id {
                            let pnl_pct = entry.to_f64().and_then(|e| {
                                mark.to_f64()
                                    .and_then(|m| (e > 0.0).then_some((m - e) / e * 100.0))
                            });
                            let outcome = if hit_tp { "profit" } else { "loss" };
                            if let Err(e) = record_decision_outcome(
                                &pool,
                                did,
                                pnl_pct,
                                None,
                                outcome,
                                None,
                                Some("sl_tp_close_dry"),
                            )
                            .await
                            {
                                warn!(%e, symbol = %key.symbol, "record_decision_outcome");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(%e, symbol = %key.symbol, "position_manager: dry SL/TP place başarısız");
                    }
                }
            } else if live_on {
                if is_trading_halted() {
                    warn!(
                        user_id = %key.user_id,
                        symbol = %key.symbol,
                        "position_manager: live close atlandı — trading halted"
                    );
                    continue;
                }
                let Some(org_id) = agg.org_id else {
                    warn!(
                        user_id = %key.user_id,
                        symbol = %key.symbol,
                        "position_manager: org_id yok — live close atlandı"
                    );
                    continue;
                };

                let sltp_place_outcome = if book_exchange_id == ExchangeId::Binance {
                    let creds = match acct_repo
                        .binance_for_user(key.user_id, key.segment.trim())
                        .await
                    {
                        Ok(c) => c,
                        Err(e) => {
                            warn!(%e, "position_manager: exchange_accounts okunamadı");
                            continue;
                        }
                    };
                    let Some(creds) = creds else {
                        warn!(
                            user_id = %key.user_id,
                            symbol = %key.symbol,
                            segment = %key.segment,
                            "position_manager: exchange_accounts yok — live close atlandı"
                        );
                        continue;
                    };
                    let seg_norm = key.segment.trim().to_lowercase();
                    let gw_cache_key = (key.user_id, seg_norm);
                    let gw = {
                        let mut guard = live_gateway_cache.lock().unwrap();
                        if let Some(g) = guard.get(&gw_cache_key) {
                            g.clone()
                        } else {
                            let cfg = BinanceClientConfig::mainnet_with_keys(
                                creds.api_key,
                                creds.api_secret,
                            );
                            let client = match BinanceClient::new(cfg) {
                                Ok(c) => Arc::new(c),
                                Err(e) => {
                                    warn!(%e, "position_manager: BinanceClient oluşturulamadı");
                                    continue;
                                }
                            };
                            let g = Arc::new(BinanceLiveGateway::new(client));
                            guard.insert(gw_cache_key, g.clone());
                            g
                        }
                    };
                    gw.place_with_venue_response(intent).await
                } else if book_exchange_id == ExchangeId::Bybit
                    && key.segment.eq_ignore_ascii_case("futures")
                {
                    let creds = match acct_repo
                        .credentials_for_user(key.user_id, "bybit", key.segment.trim())
                        .await
                    {
                        Ok(c) => c,
                        Err(e) => {
                            warn!(%e, "position_manager: exchange_accounts bybit okunamadı");
                            continue;
                        }
                    };
                    let Some(creds) = creds else {
                        warn!(
                            user_id = %key.user_id,
                            symbol = %key.symbol,
                            segment = %key.segment,
                            "position_manager: Bybit exchange_accounts missing — live close skipped"
                        );
                        continue;
                    };
                    let seg_norm = key.segment.trim().to_lowercase();
                    let gw_cache_key = (key.user_id, seg_norm.clone());
                    let gw = {
                        let mut guard = bybit_live_gateway_cache.lock().unwrap();
                        if let Some(g) = guard.get(&gw_cache_key) {
                            g.clone()
                        } else {
                            let g = Arc::new(BybitLiveGateway::mainnet(
                                creds.api_key,
                                creds.api_secret,
                            ));
                            guard.insert(gw_cache_key, g.clone());
                            g
                        }
                    };
                    gw.place_with_venue_response(intent).await
                } else if book_exchange_id == ExchangeId::Okx
                    && key.segment.eq_ignore_ascii_case("futures")
                {
                    let creds = match acct_repo
                        .credentials_for_user(key.user_id, "okx", key.segment.trim())
                        .await
                    {
                        Ok(c) => c,
                        Err(e) => {
                            warn!(%e, "position_manager: exchange_accounts okx okunamadı");
                            continue;
                        }
                    };
                    let Some(creds) = creds else {
                        warn!(
                            user_id = %key.user_id,
                            symbol = %key.symbol,
                            segment = %key.segment,
                            "position_manager: OKX exchange_accounts missing — live close skipped"
                        );
                        continue;
                    };
                    let passphrase = creds.passphrase.clone().unwrap_or_default();
                    if passphrase.trim().is_empty() {
                        warn!(
                            user_id = %key.user_id,
                            symbol = %key.symbol,
                            "position_manager: OKX passphrase missing — live close skipped",
                        );
                        continue;
                    }
                    let seg_norm = key.segment.trim().to_lowercase();
                    let gw_cache_key = (key.user_id, seg_norm.clone());
                    let gw = {
                        let mut guard = okx_live_gateway_cache.lock().unwrap();
                        if let Some(g) = guard.get(&gw_cache_key) {
                            g.clone()
                        } else {
                            let g = Arc::new(OkxLiveGateway::mainnet(
                                creds.api_key,
                                creds.api_secret,
                                passphrase,
                            ));
                            guard.insert(gw_cache_key, g.clone());
                            g
                        }
                    };
                    gw.place_with_venue_response(intent).await
                } else {
                    warn!(
                        user_id = %key.user_id,
                        symbol = %key.symbol,
                        exchange = %key.exchange,
                        ?book_exchange_id,
                        "position_manager: live SL/TP close skipped — Binance, Bybit, or OKX USDT futures only",
                    );
                    continue;
                };

                let intent_record = market_reduce_long_intent(&key, book.qty);
                match sltp_place_outcome {
                    Ok((cid, venue_json)) => {
                        let venue_oid = if book_exchange_id == ExchangeId::Binance {
                            venue_order_id_from_binance_order_response(&venue_json)
                        } else if book_exchange_id == ExchangeId::Bybit {
                            venue_order_id_from_bybit_v5_response(&venue_json)
                        } else {
                            venue_order_id_from_okx_v5_response(&venue_json)
                        };
                        match repo
                            .insert_submitted(
                                org_id,
                                key.user_id,
                                key.exchange.trim(),
                                key.segment.trim(),
                                &key.symbol,
                                cid,
                                &intent_record,
                                venue_oid,
                                Some(venue_json),
                            )
                            .await
                        {
                            Ok(_) => {
                                info!(
                                    %cid,
                                    symbol = %key.symbol,
                                    "position_manager: live reduce-only kapatma kaydedildi"
                                );
                                if let Some(did) = ai_outcome_decision_id {
                                    let pnl_pct = entry.to_f64().and_then(|e| {
                                        mark.to_f64()
                                            .and_then(|m| (e > 0.0).then_some((m - e) / e * 100.0))
                                    });
                                    let outcome = if hit_tp { "profit" } else { "loss" };
                                    if let Err(e) = record_decision_outcome(
                                        &pool,
                                        did,
                                        pnl_pct,
                                        None,
                                        outcome,
                                        None,
                                        Some("sl_tp_close_live"),
                                    )
                                    .await
                                    {
                                        warn!(%e, symbol = %key.symbol, "record_decision_outcome");
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(%e, %cid, "position_manager: live emir DB yazımı başarısız")
                            }
                        }
                    }
                    Err(e) => {
                        warn!(%e, symbol = %key.symbol, "position_manager: live place başarısız")
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    fn order_row(
        user_id: Uuid,
        org_id: Uuid,
        side: &str,
        executed_qty: &str,
        avg_price: &str,
        seq: i64,
    ) -> ExchangeOrderRow {
        let t = Utc::now() + chrono::Duration::seconds(seq);
        ExchangeOrderRow {
            id: Uuid::new_v4(),
            org_id,
            user_id,
            exchange: "binance".into(),
            segment: "spot".into(),
            symbol: "BTCUSDT".into(),
            client_order_id: Uuid::new_v4(),
            status: "filled".into(),
            intent: json!({ "side": side }),
            venue_order_id: Some(seq),
            venue_response: Some(json!({
                "executedQty": executed_qty,
                "avgPrice": avg_price,
            })),
            created_at: t,
            updated_at: t,
        }
    }

    #[test]
    fn aggregate_long_books_buy_then_partial_sell() {
        let user_id = Uuid::new_v4();
        let org_id = Uuid::new_v4();
        let rows = vec![
            order_row(user_id, org_id, "buy", "1", "100", 0),
            order_row(user_id, org_id, "sell", "0.5", "110", 1),
        ];
        let m = aggregate_long_books(&rows);
        let key = PosKey {
            user_id,
            exchange: "binance".into(),
            segment: "spot".into(),
            symbol: "BTCUSDT".into(),
        };
        let agg = m.get(&key).expect("position key");
        assert_eq!(agg.book.qty, Decimal::new(5, 1));
        assert_eq!(agg.org_id, Some(org_id));
    }

    #[test]
    fn intent_side_parses_buy_sell() {
        let buy = json!({"side": "Buy"});
        let sell = json!({"side": "SELL"});
        assert_eq!(intent_side(&buy), Some(OrderSide::Buy));
        assert_eq!(intent_side(&sell), Some(OrderSide::Sell));
    }

    #[test]
    fn position_book_exchange_id_parses_snake_case() {
        let uid = Uuid::new_v4();
        let key = |ex: &str| PosKey {
            user_id: uid,
            exchange: ex.into(),
            segment: "futures".into(),
            symbol: "BTCUSDT".into(),
        };
        assert_eq!(position_book_exchange_id(&key("binance")), ExchangeId::Binance);
        assert_eq!(position_book_exchange_id(&key("BYBIT")), ExchangeId::Bybit);
        assert_eq!(position_book_exchange_id(&key("okx")), ExchangeId::Okx);
        assert_eq!(position_book_exchange_id(&key("custom")), ExchangeId::Custom);
        assert_eq!(position_book_exchange_id(&key("")), ExchangeId::Binance);
    }
}
