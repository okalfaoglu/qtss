//! `range_signal_events` → isteğe bağlı **paper** ve/veya **Binance canlı** market emri.
//!
//! Paper: `worker.range_auto_paper_execute_enabled` / `QTSS_RANGE_AUTO_PAPER_EXECUTE_ENABLED=1` +
//! `paper_ledger_enabled` + `paper_org_id` / `paper_user_id`.
//!
//! Live (Binance only): `worker.range_auto_live_execute_enabled` +
//! `range_auto_live_org_id` / `range_auto_live_user_id` + `exchange_accounts` (binance, segment) +
//! **`QTSS_RANGE_AUTO_LIVE_CONFIRM=1`** (ortam) + `!is_trading_halted()`.
//!
//! Sıra: Binance + live koşulları (confirm, org/user, kill switch kapalı) ve `exchange_accounts` satırı varsa
//! **live** market emri; borsa `place` hatası → `failed` (paper’a düşmez).
//! Canlı denenmez veya hesap yoksa (`creds` yok) ve paper açıksa **paper** (referans fiyat gerekir).
//!
//! `app_config.range_engine.execution_gates` ile uyumlu.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use qtss_binance::{
    venue_order_id_from_binance_order_response, BinanceClient, BinanceClientConfig,
};
use qtss_common::is_trading_halted;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{
    FuturesExecutionExtras, OrderIntent, OrderSide, OrderType, TimeInForce,
};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::{BinanceLiveGateway, DryRunGateway, ExecutionGateway};
use qtss_storage::{
    default_range_engine_json, fetch_range_engine_json,
    list_range_signal_events_pending_paper_execution, resolve_system_decimal,
    resolve_system_string, resolve_worker_enabled_flag, resolve_worker_tick_secs,
    try_claim_range_signal_event_for_paper_execution, update_range_signal_paper_execution_status,
    ExchangeAccountRepository, ExchangeOrderRepository, RangeSignalEventPendingExecutionRow,
};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::strategy_runner::dry_gateway_from_pool;

fn segment_from_engine(raw: &str) -> MarketSegment {
    match raw.trim().to_lowercase().as_str() {
        "futures" | "usdt_futures" | "fapi" | "future" => MarketSegment::Futures,
        _ => MarketSegment::Spot,
    }
}

fn blocked_by_execution_gates(kind: &str, doc: &serde_json::Value) -> bool {
    let g = doc.get("execution_gates");
    let b = |key: &str, default_open: bool| {
        g.and_then(|o| o.get(key))
            .and_then(|x| x.as_bool())
            .unwrap_or(default_open)
    };
    let allow_long_open = b("allow_long_open", true);
    let allow_short_open = b("allow_short_open", true);
    let allow_all_closes = b("allow_all_closes", true);
    match kind {
        "long_entry" => !allow_long_open,
        "short_entry" => !allow_short_open,
        "long_exit" | "short_exit" => !allow_all_closes,
        _ => true,
    }
}

fn build_intent(
    row: &RangeSignalEventPendingExecutionRow,
    qty: Decimal,
) -> Result<OrderIntent, String> {
    let exchange = ExchangeId::from_str(row.exchange.trim().to_lowercase().as_str())
        .map_err(|_| format!("unsupported exchange: {}", row.exchange))?;
    let segment = segment_from_engine(&row.segment);
    let symbol = row.symbol.trim().to_uppercase();
    let side = match row.event_kind.as_str() {
        "long_entry" => OrderSide::Buy,
        "short_entry" => OrderSide::Sell,
        "long_exit" => OrderSide::Sell,
        "short_exit" => OrderSide::Buy,
        other => return Err(format!("unsupported event_kind: {other}")),
    };
    let futures = match segment {
        MarketSegment::Futures => Some(FuturesExecutionExtras {
            position_side: None,
            reduce_only: Some(matches!(
                row.event_kind.as_str(),
                "long_exit" | "short_exit"
            )),
        }),
        _ => None,
    };
    Ok(OrderIntent {
        instrument: InstrumentId {
            exchange,
            segment,
            symbol,
        },
        side,
        quantity: qty,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Ioc,
        requires_human_approval: false,
        futures,
    })
}

fn range_auto_live_env_confirm() -> bool {
    matches!(
        std::env::var("QTSS_RANGE_AUTO_LIVE_CONFIRM").ok().as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

async fn range_auto_live_target_from_db(pool: &PgPool) -> Option<(Uuid, Uuid)> {
    let org_s = resolve_system_string(
        pool,
        "worker",
        "range_auto_live_org_id",
        "QTSS_RANGE_AUTO_LIVE_ORG_ID",
        "",
    )
    .await;
    let user_s = resolve_system_string(
        pool,
        "worker",
        "range_auto_live_user_id",
        "QTSS_RANGE_AUTO_LIVE_USER_ID",
        "",
    )
    .await;
    let org = Uuid::parse_str(org_s.trim()).ok()?;
    let user = Uuid::parse_str(user_s.trim()).ok()?;
    Some((org, user))
}

pub async fn range_signal_execute_loop(pool: PgPool) {
    let dry: Arc<DryRunGateway> = dry_gateway_from_pool(&pool).await;
    let acct_repo = ExchangeAccountRepository::new(pool.clone());
    let order_repo = ExchangeOrderRepository::new(pool.clone());

    loop {
        let paper_on = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "range_auto_paper_execute_enabled",
            "QTSS_RANGE_AUTO_PAPER_EXECUTE_ENABLED",
            false,
        )
        .await;
        let live_on = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "range_auto_live_execute_enabled",
            "QTSS_RANGE_AUTO_LIVE_EXECUTE_ENABLED",
            false,
        )
        .await;

        if !paper_on && !live_on {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }

        let paper_ledger = if paper_on {
            qtss_strategy::paper_ledger_target_from_db(&pool).await
        } else {
            None
        };
        if paper_on && paper_ledger.is_none() {
            warn!(
                target: "qtss",
                qtss_module = "qtss_worker::range_signal_execute",
                "range auto paper: worker.paper_ledger_enabled + paper_org_id/paper_user_id required"
            );
        }

        let live_target = if live_on {
            range_auto_live_target_from_db(&pool).await
        } else {
            None
        };
        let live_confirm = range_auto_live_env_confirm();
        if live_on && !live_confirm {
            tracing::debug!(
                target: "qtss",
                qtss_module = "qtss_worker::range_signal_execute",
                "range auto live: QTSS_RANGE_AUTO_LIVE_CONFIRM=1 missing — live path disabled"
            );
        }
        if live_on && live_target.is_none() {
            warn!(
                target: "qtss",
                qtss_module = "qtss_worker::range_signal_execute",
                "range auto live: range_auto_live_org_id / range_auto_live_user_id (UUID) required"
            );
        }

        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "range_auto_paper_execute_tick_secs",
            "QTSS_RANGE_AUTO_PAPER_TICK_SECS",
            15,
            5,
        )
        .await;
        let max_age_hours_raw = resolve_system_string(
            &pool,
            "worker",
            "range_auto_paper_event_max_age_hours",
            "QTSS_RANGE_AUTO_PAPER_MAX_AGE_HOURS",
            "48",
        )
        .await;
        let max_age_hours: i64 = max_age_hours_raw
            .trim()
            .parse::<i64>()
            .unwrap_or(48)
            .clamp(1, 168);

        let qty_paper = resolve_system_decimal(
            &pool,
            "worker",
            "range_auto_paper_qty_base",
            "QTSS_RANGE_AUTO_PAPER_QTY_BASE",
            "0.001",
        )
        .await;
        let qty_live_raw = resolve_system_string(
            &pool,
            "worker",
            "range_auto_live_qty_base",
            "QTSS_RANGE_AUTO_LIVE_QTY_BASE",
            "",
        )
        .await;
        let qty_live = if qty_live_raw.trim().is_empty() {
            qty_paper
        } else {
            Decimal::from_str(qty_live_raw.trim()).unwrap_or(qty_paper)
        };

        let range_doc = fetch_range_engine_json(&pool)
            .await
            .unwrap_or_else(|_| default_range_engine_json());

        let pending = match list_range_signal_events_pending_paper_execution(
            &pool,
            max_age_hours,
            50,
        )
        .await
        {
            Ok(p) => p,
            Err(e) => {
                warn!(%e, "range_signal_execute: list pending");
                tokio::time::sleep(Duration::from_secs(tick)).await;
                continue;
            }
        };

        let paper_gw = paper_ledger.map(|(org_id, user_id)| {
            qtss_strategy::PaperRecordingDryGateway::new(
                Arc::clone(&dry),
                pool.clone(),
                org_id,
                user_id,
                "range_signal_auto",
            )
        });

        let live_base_ready =
            live_on && live_confirm && live_target.is_some() && !is_trading_halted();

        for row in pending {
            let claimed = match try_claim_range_signal_event_for_paper_execution(&pool, row.id).await
            {
                Ok(c) => c,
                Err(e) => {
                    warn!(%e, event_id = %row.id, "range_signal_execute: claim");
                    continue;
                }
            };
            if !claimed {
                continue;
            }

            if blocked_by_execution_gates(&row.event_kind, &range_doc) {
                let _ = update_range_signal_paper_execution_status(
                    &pool,
                    row.id,
                    "skipped_gated",
                    None,
                    Some("execution_gates"),
                )
                .await;
                continue;
            }

            let binance_row = row.exchange.trim().eq_ignore_ascii_case("binance");
            let seg = segment_from_engine(&row.segment);
            let segment_ok = matches!(seg, MarketSegment::Spot | MarketSegment::Futures);

            let try_live = live_base_ready && binance_row && segment_ok;

            let mut finished = false;

            if try_live {
                if let Some((live_org, live_user)) = live_target {
                    let intent = match build_intent(&row, qty_live) {
                        Ok(i) => i,
                        Err(msg) => {
                            let _ = update_range_signal_paper_execution_status(
                                &pool,
                                row.id,
                                "failed",
                                None,
                                Some(msg.as_str()),
                            )
                            .await;
                            continue;
                        }
                    };

                    let creds_opt = match acct_repo
                        .binance_for_user(live_user, row.segment.trim())
                        .await
                    {
                        Ok(c) => c,
                        Err(e) => {
                            let m = format!("exchange_accounts: {e}");
                            let _ = update_range_signal_paper_execution_status(
                                &pool,
                                row.id,
                                "failed",
                                None,
                                Some(m.as_str()),
                            )
                            .await;
                            continue;
                        }
                    };

                    if let Some(creds) = creds_opt {
                        let cfg =
                            BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
                        let client = match BinanceClient::new(cfg) {
                            Ok(c) => Arc::new(c),
                            Err(e) => {
                                let m = format!("binance_client: {e}");
                                let _ = update_range_signal_paper_execution_status(
                                    &pool,
                                    row.id,
                                    "failed",
                                    None,
                                    Some(m.as_str()),
                                )
                                .await;
                                continue;
                            }
                        };
                        let gw = BinanceLiveGateway::new(client);
                        let intent_record = intent.clone();
                        match gw.place_with_venue_response(intent).await {
                            Ok((cid, venue_json)) => {
                                let venue_oid =
                                    venue_order_id_from_binance_order_response(&venue_json);
                                if let Err(e) = order_repo
                                    .insert_submitted(
                                        live_org,
                                        live_user,
                                        "binance",
                                        row.segment.trim(),
                                        row.symbol.trim(),
                                        cid,
                                        &intent_record,
                                        venue_oid,
                                        Some(venue_json),
                                    )
                                    .await
                                {
                                    warn!(%e, event_id = %row.id, "range_signal_execute: insert_submitted");
                                }
                                let _ = update_range_signal_paper_execution_status(
                                    &pool,
                                    row.id,
                                    "placed_live",
                                    Some(cid),
                                    None,
                                )
                                .await;
                                info!(
                                    target: "qtss",
                                    qtss_module = "qtss_worker::range_signal_execute",
                                    %cid,
                                    event_id = %row.id,
                                    kind = %row.event_kind,
                                    symbol = %row.symbol,
                                    "range_signal Binance live market placed"
                                );
                                finished = true;
                            }
                            Err(e) => {
                                let msg = format!("{e}");
                                let _ = update_range_signal_paper_execution_status(
                                    &pool,
                                    row.id,
                                    "failed",
                                    None,
                                    Some(msg.as_str()),
                                )
                                .await;
                                warn!(
                                    target: "qtss",
                                    qtss_module = "qtss_worker::range_signal_execute",
                                    event_id = %row.id,
                                    symbol = %row.symbol,
                                    error = %msg,
                                    "range_signal live place failed"
                                );
                                finished = true;
                            }
                        }
                    }
                }
            }

            if finished {
                continue;
            }

            // Paper: non-Binance, live not applicable, halted, no keys, or creds missing
            if !paper_on || paper_gw.is_none() {
                let _ = update_range_signal_paper_execution_status(
                    &pool,
                    row.id,
                    "skipped_no_executor",
                    None,
                    Some("paper_disabled_or_no_ledger"),
                )
                .await;
                continue;
            }
            let gw = paper_gw.as_ref().expect("checked");

            let px_f = match row.reference_price {
                Some(p) if p.is_finite() && p > 0.0 => p,
                _ => {
                    let _ = update_range_signal_paper_execution_status(
                        &pool,
                        row.id,
                        "failed",
                        None,
                        Some("missing_or_invalid_reference_price"),
                    )
                    .await;
                    continue;
                }
            };
            let Some(px_dec) = Decimal::from_f64(px_f) else {
                let _ = update_range_signal_paper_execution_status(
                    &pool,
                    row.id,
                    "failed",
                    None,
                    Some("reference_price_decimal"),
                )
                .await;
                continue;
            };

            let intent = match build_intent(&row, qty_paper) {
                Ok(i) => i,
                Err(msg) => {
                    let _ = update_range_signal_paper_execution_status(
                        &pool,
                        row.id,
                        "failed",
                        None,
                        Some(msg.as_str()),
                    )
                    .await;
                    continue;
                }
            };

            if let Err(e) = gw.set_reference_price(&intent.instrument, px_dec) {
                let err_msg = format!("set_reference_price: {e}");
                let _ = update_range_signal_paper_execution_status(
                    &pool,
                    row.id,
                    "failed",
                    None,
                    Some(err_msg.as_str()),
                )
                .await;
                continue;
            }

            match gw.place(intent).await {
                Ok(cid) => {
                    let _ = update_range_signal_paper_execution_status(
                        &pool,
                        row.id,
                        "placed_paper",
                        Some(cid),
                        None,
                    )
                    .await;
                    info!(
                        target: "qtss",
                        qtss_module = "qtss_worker::range_signal_execute",
                        %cid,
                        event_id = %row.id,
                        kind = %row.event_kind,
                        symbol = %row.symbol,
                        "range_signal paper market placed"
                    );
                }
                Err(e) => {
                    let msg = format!("{e}");
                    let _ = update_range_signal_paper_execution_status(
                        &pool,
                        row.id,
                        "failed",
                        None,
                        Some(msg.as_str()),
                    )
                    .await;
                    warn!(
                        target: "qtss",
                        qtss_module = "qtss_worker::range_signal_execute",
                        event_id = %row.id,
                        symbol = %row.symbol,
                        error = %msg,
                        "range_signal paper place failed"
                    );
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}
