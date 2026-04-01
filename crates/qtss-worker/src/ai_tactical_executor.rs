//! AI Tactical Decision Execution Bridge (FAZ P2-13).
//!
//! Polls approved `ai_tactical_decisions` and opens positions when no existing
//! position is detected for the symbol. Supports both paper (dry) and live
//! execution via the standard `ExecutionGateway` trait.
//!
//! **Config flags:**
//! - `QTSS_AI_TACTICAL_EXECUTOR_ENABLED=1`      — master switch
//! - `QTSS_AI_TACTICAL_EXECUTOR_DRY=1`           — paper execution (default)
//! - `QTSS_AI_TACTICAL_EXECUTOR_LIVE=1`          — live exchange execution
//! - `QTSS_AI_TACTICAL_EXECUTOR_TICK_SECS`       — poll interval (default 30s)
//! - `QTSS_AI_TACTICAL_EXECUTOR_BASE_QTY_USDT`   — base notional per trade (default 100)

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use qtss_ai::feedback::record_decision_outcome;
use qtss_ai::storage::{
    fetch_latest_approved_tactical, mark_applied, AiRecordTable,
};
use qtss_binance::{
    venue_order_id_from_binance_order_response, BinanceClient, BinanceClientConfig,
};
use qtss_common::is_trading_halted;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{FuturesExecutionExtras, OrderIntent, OrderSide, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::{
    BinanceLiveGateway, CommissionPolicy, DryRunGateway, ExecutionGateway, VirtualLedgerParams,
};
use qtss_storage::{
    list_enabled_engine_symbols, list_recent_bars, resolve_system_decimal, resolve_system_string,
    resolve_worker_enabled_flag, resolve_worker_tick_secs, symbols_with_positive_long_from_fills,
    ExchangeAccountRepository, ExchangeOrderRepository,
};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{info, warn};

/// Directions that trigger a new long entry.
fn is_long_entry(direction: &str) -> bool {
    matches!(direction, "buy" | "strong_buy")
}

/// Directions that trigger a new short entry (futures only).
fn is_short_entry(direction: &str) -> bool {
    matches!(direction, "sell" | "strong_sell")
}

pub async fn ai_tactical_executor_loop(pool: PgPool) {
    let enabled = resolve_worker_enabled_flag(
        &pool,
        "worker",
        "ai_tactical_executor_enabled",
        "QTSS_AI_TACTICAL_EXECUTOR_ENABLED",
        false,
    )
    .await;
    if !enabled {
        info!("ai_tactical_executor disabled");
        return;
    }

    let dry_enabled = resolve_worker_enabled_flag(
        &pool,
        "worker",
        "ai_tactical_executor_dry",
        "QTSS_AI_TACTICAL_EXECUTOR_DRY",
        true,
    )
    .await;

    let live_enabled = resolve_worker_enabled_flag(
        &pool,
        "worker",
        "ai_tactical_executor_live",
        "QTSS_AI_TACTICAL_EXECUTOR_LIVE",
        false,
    )
    .await;

    if !dry_enabled && !live_enabled {
        info!("ai_tactical_executor: neither dry nor live enabled");
        return;
    }

    let dry_gateway: Option<Arc<DryRunGateway>> = if dry_enabled {
        let init = resolve_system_decimal(
            &pool,
            "worker",
            "ai_tactical_executor_quote_balance",
            "QTSS_AI_TACTICAL_EXECUTOR_QUOTE_BALANCE",
            Decimal::new(100_000, 0),
        )
        .await;
        Some(Arc::new(DryRunGateway::new(
            VirtualLedgerParams {
                initial_quote_balance: init,
            },
            CommissionPolicy::default(),
            None,
        )))
    } else {
        None
    };

    let live_on = live_enabled && dry_gateway.is_none();
    let repo = ExchangeOrderRepository::new(pool.clone());
    let acct_repo = ExchangeAccountRepository::new(pool.clone());

    info!(dry = dry_gateway.is_some(), live = live_on, "ai_tactical_executor_loop started");

    loop {
        let tick_secs = resolve_worker_tick_secs(
            &pool,
            "worker",
            "ai_tactical_executor_tick_secs",
            "QTSS_AI_TACTICAL_EXECUTOR_TICK_SECS",
            30,
            10,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(tick_secs)).await;

        if is_trading_halted() {
            continue;
        }

        // Get symbols with existing long positions (to avoid doubling).
        let filled = match repo.list_recent_filled_orders_global(2000).await {
            Ok(r) => r,
            Err(e) => {
                warn!(%e, "ai_tactical_executor: filled orders query failed");
                continue;
            }
        };
        let min_q = Decimal::new(1, 8);
        let already_long = symbols_with_positive_long_from_fills(&filled, min_q);

        // Get enabled engine symbols to know exchange/segment.
        let engine_symbols = match list_enabled_engine_symbols(&pool).await {
            Ok(s) => s,
            Err(e) => {
                warn!(%e, "ai_tactical_executor: engine symbols query failed");
                continue;
            }
        };

        for es in &engine_symbols {
            let sym = es.symbol.trim().to_uppercase();

            // Fetch approved tactical decision.
            let td = match fetch_latest_approved_tactical(&pool, &sym).await {
                Ok(Some(td)) => td,
                _ => continue,
            };

            let direction = td.direction.as_str();

            // Skip neutral / no_trade directions.
            if !is_long_entry(direction) && !is_short_entry(direction) {
                continue;
            }

            // Skip if already holding a long for this symbol (avoid doubling).
            if is_long_entry(direction) && already_long.iter().any(|s| s.eq_ignore_ascii_case(&sym)) {
                tracing::debug!(%sym, "ai_tactical_executor: already long, skipping entry");
                continue;
            }

            // Calculate order quantity.
            let base_notional = resolve_system_decimal(
                &pool,
                "worker",
                "ai_tactical_executor_base_qty_usdt",
                "QTSS_AI_TACTICAL_EXECUTOR_BASE_QTY_USDT",
                Decimal::new(100, 0),
            )
            .await;

            let multiplier = Decimal::from_f64(td.position_size_multiplier).unwrap_or(Decimal::ONE);
            let notional = base_notional * multiplier;

            // Get mark price for quantity calculation.
            let bars = list_recent_bars(
                &pool,
                &es.exchange,
                &es.segment,
                &es.symbol,
                &es.interval,
                1,
            )
            .await;
            let mark = match bars {
                Ok(ref b) if !b.is_empty() => b[0].close,
                _ => {
                    tracing::debug!(%sym, "ai_tactical_executor: no bar data");
                    continue;
                }
            };

            if mark <= Decimal::ZERO {
                continue;
            }

            let qty = (notional / mark).round_dp(6);
            if qty <= Decimal::ZERO {
                continue;
            }

            // Build OrderIntent.
            let exchange_id = ExchangeId::from_str(es.exchange.trim())
                .unwrap_or(ExchangeId::Binance);
            let segment = if es.segment.eq_ignore_ascii_case("futures") {
                MarketSegment::Futures
            } else {
                MarketSegment::Spot
            };

            let side = if is_long_entry(direction) {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            };

            let futures = if segment == MarketSegment::Futures {
                Some(FuturesExecutionExtras {
                    position_side: None,
                    reduce_only: Some(false),
                })
            } else {
                None
            };

            let intent = OrderIntent {
                instrument: InstrumentId {
                    exchange: exchange_id,
                    segment,
                    symbol: sym.clone(),
                },
                side,
                quantity: qty,
                order_type: OrderType::Market,
                time_in_force: TimeInForce::Gtc,
                requires_human_approval: false,
                futures,
            };

            // Execute: dry or live.
            if let Some(ref gw) = dry_gateway {
                if let Err(e) = gw.set_reference_price(&intent.instrument, mark) {
                    warn!(%e, %sym, "ai_tactical_executor: dry set_reference_price");
                    continue;
                }
                match gw.place(intent).await {
                    Ok(cid) => {
                        info!(
                            %cid, %sym, %direction, %qty, multiplier = %td.position_size_multiplier,
                            "ai_tactical_executor: dry entry placed"
                        );
                        let _ = mark_applied(&pool, AiRecordTable::TacticalChild, td.id).await;
                    }
                    Err(e) => {
                        warn!(%e, %sym, "ai_tactical_executor: dry place failed");
                    }
                }
            } else if live_on {
                if is_trading_halted() {
                    warn!(%sym, "ai_tactical_executor: trading halted, skipping live entry");
                    continue;
                }

                // Resolve org_id from engine_symbols or first filled order.
                let org_id = filled
                    .iter()
                    .find(|r| r.symbol.eq_ignore_ascii_case(&sym))
                    .map(|r| r.org_id);
                let Some(org_id) = org_id else {
                    warn!(%sym, "ai_tactical_executor: no org_id found for live execution");
                    continue;
                };

                // Resolve user_id.
                let user_id = filled
                    .iter()
                    .find(|r| r.symbol.eq_ignore_ascii_case(&sym))
                    .map(|r| r.user_id);
                let Some(user_id) = user_id else {
                    warn!(%sym, "ai_tactical_executor: no user_id found");
                    continue;
                };

                let seg_str = if segment == MarketSegment::Futures {
                    "futures"
                } else {
                    "spot"
                };

                match acct_repo.binance_for_user(user_id, seg_str).await {
                    Ok(Some(creds)) => {
                        let cfg = match BinanceClientConfig::mainnet_with_keys(
                            creds.api_key.clone(),
                            creds.api_secret.clone(),
                        ) {
                            Ok(c) => c,
                            Err(e) => {
                                warn!(%e, %sym, "ai_tactical_executor: BinanceClientConfig failed");
                                continue;
                            }
                        };
                        let client = Arc::new(BinanceClient::new(cfg));
                        let gw = BinanceLiveGateway::new(client);
                        match gw.place_with_venue_response(intent.clone()).await {
                            Ok((cid, venue_json)) => {
                                let venue_oid =
                                    venue_order_id_from_binance_order_response(&venue_json);
                                if let Err(e) = repo
                                    .insert_submitted(
                                        org_id,
                                        user_id,
                                        "binance",
                                        seg_str,
                                        &sym,
                                        cid,
                                        &intent,
                                        venue_oid,
                                        Some(venue_json),
                                    )
                                    .await
                                {
                                    warn!(%e, %cid, "ai_tactical_executor: DB insert failed");
                                }
                                info!(
                                    %cid, %sym, %direction, %qty,
                                    "ai_tactical_executor: live entry placed"
                                );
                                let _ =
                                    mark_applied(&pool, AiRecordTable::TacticalChild, td.id).await;
                            }
                            Err(e) => {
                                warn!(%e, %sym, "ai_tactical_executor: live place failed");
                            }
                        }
                    }
                    Ok(None) => {
                        warn!(%sym, "ai_tactical_executor: no Binance credentials");
                    }
                    Err(e) => {
                        warn!(%e, %sym, "ai_tactical_executor: exchange_accounts query failed");
                    }
                }
            }
        }
    }
}
