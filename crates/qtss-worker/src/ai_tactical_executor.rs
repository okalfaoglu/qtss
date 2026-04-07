//! AI Tactical Decision Execution Bridge (FAZ P2-13).
//!
//! Polls approved `ai_tactical_decisions` and opens positions when no existing
//! position (long or short) is detected for the symbol.
//!
//! ## Market support
//! - **Spot**: Long entries only (`buy`/`strong_buy`). Short selling not supported.
//! - **Futures (USDT-M)**: Bidirectional — long (`buy`/`strong_buy`) and short
//!   (`sell`/`strong_sell`). Leverage is set per-symbol before order placement.
//!
//! ## Commission
//! - **Dry mode**: Uses `CommissionPolicy` (fixed bps or exchange API quote).
//!   Configurable via `QTSS_AI_TACTICAL_EXECUTOR_MAKER_BPS` / `TAKER_BPS`.
//!   Simulated fills are written to `exchange_orders` (no Binance request) when
//!   `QTSS_AI_TACTICAL_EXECUTOR_ORG_ID` / `QTSS_AI_TACTICAL_EXECUTOR_USER_ID` are set
//!   or a prior order row supplies org/user for the symbol.
//! - **Live mode**: Exchange native commission applies; no local override needed.
//!
//! ## Config flags
//! - `QTSS_AI_TACTICAL_EXECUTOR_ENABLED=1`          — master switch
//! - `QTSS_AI_TACTICAL_EXECUTOR_DRY=1`              — paper execution (default)
//! - `QTSS_AI_TACTICAL_EXECUTOR_LIVE=1`             — live exchange execution
//! - `QTSS_AI_TACTICAL_EXECUTOR_TICK_SECS`          — poll interval (default 30s)
//! - `QTSS_AI_TACTICAL_EXECUTOR_BASE_QTY_USDT`     — base notional per trade (default 100)
//! - `QTSS_AI_TACTICAL_EXECUTOR_QUOTE_BALANCE`      — dry ledger initial balance (default 100,000)
//! - `QTSS_AI_TACTICAL_EXECUTOR_LEVERAGE`            — futures leverage (default 1)
//! - `QTSS_AI_TACTICAL_EXECUTOR_MAKER_BPS`           — dry commission maker bps (default 2)
//! - `QTSS_AI_TACTICAL_EXECUTOR_TAKER_BPS`           — dry commission taker bps (default 5)

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use qtss_ai::storage::{
    fetch_latest_approved_tactical, mark_applied, AiRecordTable, AiTacticalDecisionRow,
};
use qtss_binance::{
    venue_order_id_from_binance_order_response, BinanceClient, BinanceClientConfig,
};
use qtss_common::is_trading_halted;
use qtss_domain::commission::CommissionPolicy;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{FuturesExecutionExtras, OrderIntent, OrderSide, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::{
    BinanceLiveGateway, DryRunGateway, ExecutionGateway, VirtualLedgerParams,
};
use qtss_storage::{
    list_enabled_engine_symbols, list_recent_bars, resolve_system_decimal,
    resolve_worker_enabled_flag, resolve_worker_tick_secs, ExchangeAccountRepository,
    ExchangeOrderRepository, ExchangeOrderRow, EngineSymbolRow,
};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Direction helpers
// ---------------------------------------------------------------------------

fn is_long_entry(direction: &str) -> bool {
    matches!(direction, "buy" | "strong_buy")
}

fn is_short_entry(direction: &str) -> bool {
    matches!(direction, "sell" | "strong_sell")
}

fn is_actionable(direction: &str) -> bool {
    is_long_entry(direction) || is_short_entry(direction)
}

// ---------------------------------------------------------------------------
// Position detection — aggregate from filled orders
// ---------------------------------------------------------------------------

/// Returns (net_long_qty, net_short_qty) for `symbol`.  Both are >= 0.
/// net_long_qty > 0 means an open long; net_short_qty > 0 means an open short.
fn net_position_for_symbol(filled: &[ExchangeOrderRow], symbol: &str) -> (Decimal, Decimal) {
    let sym = symbol.trim().to_uppercase();
    let mut net = Decimal::ZERO;
    for row in filled {
        if !row.symbol.eq_ignore_ascii_case(&sym) {
            continue;
        }
        let side = row.intent.get("side").and_then(|v| v.as_str()).unwrap_or("");
        let qty = row
            .intent
            .get("quantity")
            .and_then(|v| v.as_str())
            .and_then(|s| Decimal::from_str(s).ok())
            .or_else(|| {
                row.intent
                    .get("quantity")
                    .and_then(|v| v.as_f64())
                    .and_then(Decimal::from_f64)
            })
            .unwrap_or(Decimal::ZERO);
        match side {
            "buy" | "Buy" => net += qty,
            "sell" | "Sell" => net -= qty,
            _ => {}
        }
    }
    if net > Decimal::ZERO {
        (net, Decimal::ZERO)
    } else if net < Decimal::ZERO {
        (Decimal::ZERO, net.abs())
    } else {
        (Decimal::ZERO, Decimal::ZERO)
    }
}

// ---------------------------------------------------------------------------
// Order intent builder
// ---------------------------------------------------------------------------

fn build_entry_intent(
    exchange_id: ExchangeId,
    segment: MarketSegment,
    symbol: &str,
    side: OrderSide,
    qty: Decimal,
) -> OrderIntent {
    let futures = match segment {
        MarketSegment::Futures => Some(FuturesExecutionExtras {
            position_side: None,
            reduce_only: Some(false),
        }),
        _ => None,
    };
    OrderIntent {
        instrument: InstrumentId {
            exchange: exchange_id,
            segment,
            symbol: symbol.to_string(),
        },
        side,
        quantity: qty,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Gtc,
        requires_human_approval: false,
        futures,
    }
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

struct ExecutorConfig {
    dry_enabled: bool,
    live_enabled: bool,
    base_notional: Decimal,
    leverage: u32,
    maker_bps: u32,
    taker_bps: u32,
    quote_balance: Decimal,
}

async fn load_executor_config(pool: &PgPool) -> ExecutorConfig {
    let dry_enabled = resolve_worker_enabled_flag(
        pool, "worker", "ai_tactical_executor_dry", "QTSS_AI_TACTICAL_EXECUTOR_DRY", true,
    ).await;
    let live_enabled = resolve_worker_enabled_flag(
        pool, "worker", "ai_tactical_executor_live", "QTSS_AI_TACTICAL_EXECUTOR_LIVE", false,
    ).await;
    let base_notional = resolve_system_decimal(
        pool, "worker", "ai_tactical_executor_base_qty_usdt",
        "QTSS_AI_TACTICAL_EXECUTOR_BASE_QTY_USDT", Decimal::new(100, 0),
    ).await;
    let leverage = std::env::var("QTSS_AI_TACTICAL_EXECUTOR_LEVERAGE")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(1)
        .clamp(1, 125);
    let maker_bps = std::env::var("QTSS_AI_TACTICAL_EXECUTOR_MAKER_BPS")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(2);
    let taker_bps = std::env::var("QTSS_AI_TACTICAL_EXECUTOR_TAKER_BPS")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(5);
    let quote_balance = resolve_system_decimal(
        pool, "worker", "ai_tactical_executor_quote_balance",
        "QTSS_AI_TACTICAL_EXECUTOR_QUOTE_BALANCE", Decimal::new(100_000, 0),
    ).await;
    ExecutorConfig {
        dry_enabled,
        live_enabled,
        base_notional,
        leverage,
        maker_bps,
        taker_bps,
        quote_balance,
    }
}

// ---------------------------------------------------------------------------
// Dry execution
// ---------------------------------------------------------------------------

async fn execute_dry(
    gw: &DryRunGateway,
    pool: &PgPool,
    repo: &ExchangeOrderRepository,
    intent: &OrderIntent,
    mark: Decimal,
    td: &AiTacticalDecisionRow,
    sym: &str,
    direction: &str,
    org_user: Option<(Uuid, Uuid)>,
    exchange_slug: &str,
    segment_db: &str,
) {
    if let Err(e) = gw.set_reference_price(&intent.instrument, mark) {
        warn!(%e, %sym, "ai_tactical_executor: dry set_reference_price");
        return;
    }
    let out = match gw.place_detailed(intent.clone(), None) {
        Ok(o) => o,
        Err(e) => {
            warn!(%e, %sym, "ai_tactical_executor: dry place failed");
            return;
        }
    };
    let cid = out.client_order_id;

    if let Some((org_id, user_id)) = org_user {
        let venue_response = json!({
            "dry_run": true,
            "simulation_source": "ai_tactical_executor",
            "status": "FILLED",
            "executedQty": out.fill.quantity.to_string(),
            "avgPrice": out.fill.avg_price.to_string(),
            "fee": out.fill.fee.to_string(),
            "ai_tactical_decision_row_id": td.id,
            "ai_decision_id": td.decision_id,
            "direction": direction,
            "note": "Simulated fill; no HTTP request sent to the exchange.",
        });
        match repo
            .insert_submitted(
                org_id,
                user_id,
                exchange_slug,
                segment_db,
                sym,
                cid,
                intent,
                None,
                Some(venue_response),
            )
            .await
        {
            Ok(row) => {
                info!(
                    %cid,
                    exchange_order_row_id = %row.id,
                    %sym,
                    %direction,
                    qty = %out.fill.quantity,
                    "ai_tactical_executor: dry simulated order persisted to exchange_orders"
                );
            }
            Err(e) => {
                warn!(%e, %cid, %sym, "ai_tactical_executor: dry exchange_orders insert failed");
            }
        }
    } else {
        warn!(
            %sym,
            %cid,
            "ai_tactical_executor: dry fill in-memory only — set QTSS_AI_TACTICAL_EXECUTOR_ORG_ID and QTSS_AI_TACTICAL_EXECUTOR_USER_ID (or place a live order for this symbol first) to persist exchange_orders"
        );
    }

    info!(
        %cid,
        %sym,
        %direction,
        qty = %out.fill.quantity,
        multiplier = %td.position_size_multiplier,
        "ai_tactical_executor: dry entry placed"
    );
    let _ = mark_applied(pool, AiRecordTable::TacticalChild, td.id).await;
}

// ---------------------------------------------------------------------------
// Live execution (Binance)
// ---------------------------------------------------------------------------

async fn execute_live_binance(
    pool: &PgPool,
    repo: &ExchangeOrderRepository,
    acct_repo: &ExchangeAccountRepository,
    intent: &OrderIntent,
    td: &AiTacticalDecisionRow,
    sym: &str,
    direction: &str,
    qty: Decimal,
    segment: MarketSegment,
    leverage: u32,
    org_id: Uuid,
    user_id: Uuid,
) {
    if is_trading_halted() {
        warn!(%sym, "ai_tactical_executor: trading halted — skipping live entry");
        return;
    }

    let seg_str = match segment {
        MarketSegment::Futures => "futures",
        _ => "spot",
    };

    let creds = match acct_repo.binance_for_user(user_id, seg_str).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            warn!(%sym, %seg_str, "ai_tactical_executor: no Binance credentials");
            return;
        }
        Err(e) => {
            warn!(%e, %sym, "ai_tactical_executor: exchange_accounts query failed");
            return;
        }
    };

    let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key.clone(), creds.api_secret.clone());
    let client = match BinanceClient::new(cfg) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            warn!(%e, %sym, "ai_tactical_executor: BinanceClient creation failed");
            return;
        }
    };

    // Set leverage for futures before placing the order.
    if segment == MarketSegment::Futures && leverage > 1 {
        match client.fapi_change_leverage(sym, leverage).await {
            Ok(_) => {
                info!(%sym, %leverage, "ai_tactical_executor: leverage set");
            }
            Err(e) => {
                warn!(%e, %sym, %leverage, "ai_tactical_executor: leverage set failed — proceeding with current leverage");
            }
        }
    }

    let gw = BinanceLiveGateway::new(client);
    match gw.place_with_venue_response(intent.clone()).await {
        Ok((cid, venue_json)) => {
            let venue_oid = venue_order_id_from_binance_order_response(&venue_json);
            if let Err(e) = repo
                .insert_submitted(
                    org_id, user_id, "binance", seg_str, sym, cid, intent, venue_oid,
                    Some(venue_json),
                )
                .await
            {
                warn!(%e, %cid, "ai_tactical_executor: DB insert failed");
            }
            info!(
                %cid, %sym, %direction, %qty, %leverage,
                "ai_tactical_executor: live entry placed"
            );
            let _ = mark_applied(pool, AiRecordTable::TacticalChild, td.id).await;
        }
        Err(e) => {
            warn!(%e, %sym, "ai_tactical_executor: live place failed");
        }
    }
}

// ---------------------------------------------------------------------------
// Resolve org_id / user_id from filled orders or engine config
// ---------------------------------------------------------------------------

fn resolve_user_org(filled: &[ExchangeOrderRow], sym: &str) -> Option<(Uuid, Uuid)> {
    filled
        .iter()
        .find(|r| r.symbol.eq_ignore_ascii_case(sym))
        .map(|r| (r.org_id, r.user_id))
        .or_else(|| {
            // Fallback: try env for default account
            let org = std::env::var("QTSS_AI_TACTICAL_EXECUTOR_ORG_ID")
                .ok()
                .and_then(|s| Uuid::parse_str(s.trim()).ok());
            let user = std::env::var("QTSS_AI_TACTICAL_EXECUTOR_USER_ID")
                .ok()
                .and_then(|s| Uuid::parse_str(s.trim()).ok());
            match (org, user) {
                (Some(o), Some(u)) => Some((o, u)),
                _ => None,
            }
        })
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

pub async fn ai_tactical_executor_loop(pool: PgPool) {
    let enabled = resolve_worker_enabled_flag(
        &pool, "worker", "ai_tactical_executor_enabled",
        "QTSS_AI_TACTICAL_EXECUTOR_ENABLED", false,
    ).await;
    if !enabled {
        info!("ai_tactical_executor disabled");
        return;
    }

    let ecfg = load_executor_config(&pool).await;
    if !ecfg.dry_enabled && !ecfg.live_enabled {
        info!("ai_tactical_executor: neither dry nor live enabled");
        return;
    }

    let commission_policy = CommissionPolicy::ManualBps {
        maker_bps: ecfg.maker_bps,
        taker_bps: ecfg.taker_bps,
    };

    let dry_gateway: Option<Arc<DryRunGateway>> = if ecfg.dry_enabled {
        Some(Arc::new(DryRunGateway::new(
            VirtualLedgerParams { initial_quote_balance: ecfg.quote_balance },
            commission_policy,
            None,
        )))
    } else {
        None
    };

    let live_on = ecfg.live_enabled && dry_gateway.is_none();
    let repo = ExchangeOrderRepository::new(pool.clone());
    let acct_repo = ExchangeAccountRepository::new(pool.clone());

    info!(
        dry = dry_gateway.is_some(), live = live_on,
        leverage = ecfg.leverage,
        base_notional = %ecfg.base_notional,
        maker_bps = ecfg.maker_bps,
        taker_bps = ecfg.taker_bps,
        "ai_tactical_executor_loop started"
    );

    loop {
        let tick_secs = resolve_worker_tick_secs(
            &pool, "worker", "ai_tactical_executor_tick_secs",
            "QTSS_AI_TACTICAL_EXECUTOR_TICK_SECS", 30, 10,
        ).await;
        tokio::time::sleep(Duration::from_secs(tick_secs)).await;

        if is_trading_halted() {
            continue;
        }

        // Load filled orders for position detection.
        let filled = match repo.list_recent_filled_orders_global(2000).await {
            Ok(r) => r,
            Err(e) => {
                warn!(%e, "ai_tactical_executor: filled orders query failed");
                continue;
            }
        };

        let engine_symbols = match list_enabled_engine_symbols(&pool).await {
            Ok(s) => s,
            Err(e) => {
                warn!(%e, "ai_tactical_executor: engine symbols query failed");
                continue;
            }
        };

        for es in &engine_symbols {
            let sym = es.symbol.trim().to_uppercase();
            let segment = parse_segment(&es.segment);

            // Fetch approved tactical decision.
            let td = match fetch_latest_approved_tactical(&pool, &sym).await {
                Ok(Some(td)) => td,
                _ => continue,
            };

            let direction = td.direction.as_str();
            if !is_actionable(direction) {
                continue;
            }

            // Block short entries on spot markets.
            if is_short_entry(direction) && segment != MarketSegment::Futures {
                tracing::debug!(
                    %sym, %direction,
                    "ai_tactical_executor: short entry not supported on spot — skipping"
                );
                continue;
            }

            // Check existing positions to avoid doubling.
            let (long_qty, short_qty) = net_position_for_symbol(&filled, &sym);
            let min_q = Decimal::new(1, 8);

            if is_long_entry(direction) && long_qty > min_q {
                tracing::debug!(%sym, %long_qty, "ai_tactical_executor: already long — skipping");
                continue;
            }
            if is_short_entry(direction) && short_qty > min_q {
                tracing::debug!(%sym, %short_qty, "ai_tactical_executor: already short — skipping");
                continue;
            }

            // Calculate quantity.
            let multiplier = Decimal::from_f64(td.position_size_multiplier).unwrap_or(Decimal::ONE);
            let effective_leverage = Decimal::from(ecfg.leverage);
            let notional = ecfg.base_notional * multiplier * effective_leverage;

            let mark = match get_mark_price(&pool, es).await {
                Some(m) if m > Decimal::ZERO => m,
                _ => {
                    tracing::debug!(%sym, "ai_tactical_executor: no bar data");
                    continue;
                }
            };

            let qty = (notional / mark).round_dp(6);
            if qty <= Decimal::ZERO {
                continue;
            }

            // Build intent.
            let exchange_id = ExchangeId::from_str(es.exchange.trim()).unwrap_or(ExchangeId::Binance);
            let side = if is_long_entry(direction) { OrderSide::Buy } else { OrderSide::Sell };
            let intent = build_entry_intent(exchange_id, segment, &sym, side, qty);

            // Execute.
            if let Some(ref gw) = dry_gateway {
                let org_user = resolve_user_org(&filled, &sym);
                let exchange_slug = es.exchange.trim();
                let segment_db = match segment {
                    MarketSegment::Futures => "futures",
                    _ => "spot",
                };
                execute_dry(
                    gw,
                    &pool,
                    &repo,
                    &intent,
                    mark,
                    &td,
                    &sym,
                    direction,
                    org_user,
                    exchange_slug,
                    segment_db,
                )
                .await;
            } else if live_on {
                let Some((org_id, user_id)) = resolve_user_org(&filled, &sym) else {
                    warn!(%sym, "ai_tactical_executor: no org_id/user_id for live execution");
                    continue;
                };
                execute_live_binance(
                    &pool, &repo, &acct_repo, &intent, &td, &sym, direction, qty,
                    segment, ecfg.leverage, org_id, user_id,
                ).await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_segment(s: &str) -> MarketSegment {
    if s.eq_ignore_ascii_case("futures") {
        MarketSegment::Futures
    } else {
        MarketSegment::Spot
    }
}

async fn get_mark_price(pool: &PgPool, es: &EngineSymbolRow) -> Option<Decimal> {
    let bars = list_recent_bars(pool, &es.exchange, &es.segment, &es.symbol, &es.interval, 1)
        .await
        .ok()?;
    bars.into_iter().next().map(|b| b.close)
}
