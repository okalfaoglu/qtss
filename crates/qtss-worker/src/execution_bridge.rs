//! Faz 9.8.11 — execution bridge worker.
//!
//! Claims rows from `selected_candidates` (FOR UPDATE SKIP LOCKED) and
//! dispatches them.  This version lands a *minimal viable dispatch*:
//!
//! - Dry mode: insert a row into `exchange_orders` flagged as a paper
//!   fill (venue_order_id = NULL, status = 'filled'), so the dry GUI
//!   and downstream analytics (PnL, Training Set, AI Shadow) see real
//!   rows for every selected candidate.
//! - Live mode: gated off by default (`execution.live.enabled=false`).
//!   When flipped on a future patch will wire the broker gateway; for
//!   now live rows are marked `errored` with a clear message so they
//!   don't sit in `pending` indefinitely.
//!
//! Keeping the bridge thin on purpose: the heavy lifting (order sizing,
//! slippage guard, liquidation guard) already ran upstream in the
//! allocator and risk engine. The bridge's job is to *close the loop*
//! so the GUI pages (Training Set, Model Registry, AI Shadow, live
//! positions) stop looking like abandoned skeletons.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use qtss_binance::{BinanceClient, BinanceClientConfig};
use qtss_domain::exchange::{ExchangeId, MarketSegment as DomainSegment};
use qtss_domain::orders::{
    FuturesExecutionExtras, OrderIntent, OrderSide, OrderType, TimeInForce,
};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::BinanceLiveGateway;
use qtss_risk::{
    ExecutionMode, LivePositionState, LivePositionStore, MarketSegment, PositionSide, TpLeg,
};
use qtss_storage::{
    claim_selected_candidates, insert_live_position, mark_selected_errored, mark_selected_placed,
    resolve_account_equity_usd, resolve_system_f64, resolve_system_string, resolve_system_u64,
    resolve_worker_enabled_flag, ExchangeAccountRepository, ExchangeOrderRepository,
    InsertLivePosition, SelectedCandidateRow,
};
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

const MODULE: &str = "execution";
const CFG_INTERVAL_MS: &str = "execution.loop_interval_ms";
const CFG_DRY_ENABLED: &str = "execution.dry.enabled";
const CFG_LIVE_ENABLED: &str = "execution.live.enabled";
const ENV_INTERVAL: &str = "QTSS_EXEC_BRIDGE_INTERVAL_MS";
const ENV_DRY: &str = "QTSS_EXEC_DRY_ENABLED";
const ENV_LIVE: &str = "QTSS_EXEC_LIVE_ENABLED";

const DEFAULT_INTERVAL_MS: u64 = 2_000;
const BATCH: i64 = 10;

pub async fn execution_bridge_loop(pool: PgPool, store: Arc<LivePositionStore>) {
    info!("execution bridge worker: starting");
    loop {
        let interval_ms = resolve_system_u64(
            &pool, MODULE, CFG_INTERVAL_MS, ENV_INTERVAL,
            DEFAULT_INTERVAL_MS, 500, 600_000,
        )
        .await;
        if let Err(e) = run_tick(&pool, &store).await {
            warn!(error=%e, "execution bridge tick failed");
        }
        tokio::time::sleep(Duration::from_millis(interval_ms.max(500))).await;
    }
}

async fn run_tick(
    pool: &PgPool,
    store: &LivePositionStore,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dry_enabled =
        resolve_worker_enabled_flag(pool, MODULE, CFG_DRY_ENABLED, ENV_DRY, true).await;
    let live_enabled =
        resolve_worker_enabled_flag(pool, MODULE, CFG_LIVE_ENABLED, ENV_LIVE, false).await;
    let rows = claim_selected_candidates(pool, BATCH).await?;
    if rows.is_empty() {
        return Ok(());
    }
    for row in rows {
        let outcome = dispatch(pool, store, &row, dry_enabled, live_enabled).await;
        match outcome {
            Ok(()) => mark_selected_placed(pool, row.id).await?,
            Err(e) => {
                let msg = e.to_string();
                warn!(id = row.id, setup = %row.setup_id, error = %msg, "candidate dispatch failed");
                mark_selected_errored(pool, row.id, &msg).await?;
                // When a LIVE dispatch fails (broker rejected order, key
                // has no trading permission, live_enabled=false, …) the
                // setup must not remain `armed` — otherwise setup_watcher
                // will later mark it `closed_loss` on an SL touch without
                // any real broker position ever having been opened. Flip
                // the setup row to `rejected` so the lifecycle accounting
                // reflects that zero capital was ever at risk.
                if row.mode == "live" {
                    if let Err(upd) = mark_setup_rejected(pool, row.setup_id, &msg).await {
                        warn!(setup = %row.setup_id, error = %upd, "mark_setup_rejected failed");
                    }
                }
            }
        }
    }
    Ok(())
}

/// Terminal stamp for a setup whose live dispatch never placed a real
/// broker order. Writes `state='rejected'`, `closed_at=now()`, and
/// records the reason so the Setups page / RADAR reports can tell the
/// difference between "closed on SL" and "never opened".
async fn mark_setup_rejected(
    pool: &PgPool,
    setup_id: Uuid,
    reason: &str,
) -> Result<(), sqlx::Error> {
    // `close_reason` is constrained to the lifecycle vocabulary
    // (tp_final / sl_hit / trail_stop / invalidated / cancelled / ...) —
    // use 'cancelled' (the canonical "never opened" terminator) and
    // stash the raw dispatch error under `raw_meta.rejected_reason`
    // so operators can still see *why* the setup was cancelled.
    sqlx::query(
        r#"UPDATE qtss_setups
              SET state        = 'rejected',
                  close_reason = 'cancelled',
                  closed_at    = now(),
                  updated_at   = now(),
                  raw_meta     = raw_meta ||
                                 jsonb_build_object('rejected_reason', $2::text)
            WHERE id = $1
              AND closed_at IS NULL"#,
    )
    .bind(setup_id)
    .bind(reason)
    .execute(pool)
    .await
    .map(|_| ())
}

async fn dispatch(
    pool: &PgPool,
    store: &LivePositionStore,
    row: &SelectedCandidateRow,
    dry_enabled: bool,
    live_enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match row.mode.as_str() {
        "dry" if dry_enabled => dispatch_dry(pool, store, row).await,
        "dry" => Err("dry execution disabled via config".into()),
        "live" if live_enabled => dispatch_live(pool, store, row).await,
        "live" => Err("live execution disabled via config".into()),
        "backtest" => Ok(()), // backtest rows are consumed by the backtest runner, not the bridge
        other => Err(format!("unknown mode: {other}").into()),
    }
}

async fn dispatch_dry(
    pool: &PgPool,
    store: &LivePositionStore,
    row: &SelectedCandidateRow,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Canonical source of truth for a paper fill is `live_positions` —
    // the tick dispatcher evaluates it, the GUI lists it, the outcome
    // labeler reads it for training set ground truth. We intentionally
    // skip `exchange_orders` because that table is reserved for rows
    // placed through the broker adapter (it requires org/user/intent
    // jsonb shapes tied to real orders); paper fills would only clutter
    // its audit trail.
    // Populate live_positions so the tick dispatcher / GUI see the
    // open paper position. Failure here is non-fatal — the paper order
    // is already recorded; surface via warn! so we notice the drift.
    // On success we also upsert into the in-memory LivePositionStore so
    // the tick dispatcher can start evaluating it on the very next
    // sweep, without waiting for the 60s re-hydrate cadence.
    if let Some(live) = build_live_position_for_mode(pool, row, "dry").await {
        match insert_live_position(pool, &live).await {
            Ok(lp_id) => {
                debug!(setup = %row.setup_id, live_pos = %lp_id, "dry live_positions ok");
                if let Some(state) = build_live_state(lp_id, &live) {
                    store.upsert(state);
                }
            }
            Err(e) => warn!(setup = %row.setup_id, error = %e, "live_positions insert failed"),
        }
    } else {
        warn!(setup = %row.setup_id, "skipping live_positions: system org/user unresolved");
    }

    debug!(setup = %row.setup_id, "dry dispatch ok");
    Ok(())
}

/// Faz 9.8.17 — live dispatch: place a real order via Binance gateway,
/// persist `exchange_orders` + `live_positions(mode='live')`, and mirror
/// into the in-memory store so the tick dispatcher starts tracking it
/// on the next sweep.
///
/// Design notes:
/// - Credentials come from `exchange_accounts` keyed by (user_id,
///   'binance', <segment>). The default user/org are config keys — on
///   multi-tenant deploys the selector will carry a concrete user_id
///   on the candidate row; single-tenant dev deploys fall back to the
///   configured default.
/// - Quantity is a placeholder (0.01) — upstream sizing (risk allocator)
///   is the authoritative source once the live adapter is exercised by
///   real setups. Faz 9.8.18 will plumb `risk_pct` → qty.
/// - Order type is Market for now; OCO SL/TP legs are a later step.
async fn dispatch_live(
    pool: &PgPool,
    store: &LivePositionStore,
    row: &SelectedCandidateRow,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let live = build_live_position_for_mode(pool, row, "live")
        .await
        .ok_or_else(|| "live dispatch: system org/user unresolved".to_string())?;

    // Credentials
    let accounts = ExchangeAccountRepository::new(pool.clone());
    let creds = accounts
        .binance_for_user(live.user_id, &live.segment)
        .await?
        .ok_or_else(|| {
            format!(
                "no exchange_accounts row for user={} exchange=binance segment={}",
                live.user_id, live.segment
            )
        })?;

    // Gateway
    let client_cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
    let client = std::sync::Arc::new(BinanceClient::new(client_cfg)?);
    let gateway = BinanceLiveGateway::new(client);

    // OrderIntent
    let intent = build_order_intent(&live)?;

    // Place
    let (client_order_id, venue_response) = gateway.place_with_venue_response(intent.clone()).await?;
    let venue_order_id = venue_response
        .get("orderId")
        .and_then(|v| v.as_i64());

    // Persist exchange_orders
    let orders = ExchangeOrderRepository::new(pool.clone());
    orders
        .insert_submitted(
            live.org_id,
            live.user_id,
            &live.exchange,
            &live.segment,
            &live.symbol,
            client_order_id,
            &intent,
            venue_order_id,
            Some(venue_response),
        )
        .await?;

    // Persist live_positions + upsert store
    match insert_live_position(pool, &live).await {
        Ok(lp_id) => {
            info!(setup = %row.setup_id, live_pos = %lp_id, cid = %client_order_id, "live order placed");
            if let Some(state) = build_live_state(lp_id, &live) {
                store.upsert(state);
            }
        }
        Err(e) => warn!(setup = %row.setup_id, error = %e, "live_positions insert failed"),
    }
    Ok(())
}

fn build_order_intent(
    live: &InsertLivePosition,
) -> Result<OrderIntent, Box<dyn std::error::Error + Send + Sync>> {
    let segment = match live.segment.as_str() {
        "spot" => DomainSegment::Spot,
        "futures" => DomainSegment::Futures,
        "margin" => DomainSegment::Margin,
        "options" => DomainSegment::Options,
        other => return Err(format!("unsupported segment for live: {other}").into()),
    };
    let side = match live.side {
        "BUY" | "buy" => OrderSide::Buy,
        "SELL" | "sell" => OrderSide::Sell,
        other => return Err(format!("unknown side: {other}").into()),
    };
    let futures_extras = if matches!(segment, DomainSegment::Futures) {
        Some(FuturesExecutionExtras {
            position_side: None, // one-way mode; hedge support later
            reduce_only: Some(false),
        })
    } else {
        None
    };
    Ok(OrderIntent {
        instrument: InstrumentId {
            exchange: ExchangeId::Binance,
            segment,
            symbol: live.symbol.clone(),
        },
        side,
        quantity: live.qty_filled,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Gtc,
        requires_human_approval: false,
        futures: futures_extras,
    })
}

fn build_live_state(id: Uuid, p: &InsertLivePosition) -> Option<LivePositionState> {
    let mode = match p.mode {
        "dry" => ExecutionMode::Dry,
        "live" => ExecutionMode::Live,
        _ => return None,
    };
    let segment = MarketSegment::parse(&p.segment)?;
    let side = match p.side {
        "BUY" | "buy" => PositionSide::Buy,
        "SELL" | "sell" => PositionSide::Sell,
        _ => return None,
    };
    let tp_ladder: Vec<TpLeg> = serde_json::from_value(p.tp_ladder.clone()).unwrap_or_default();
    let leverage: u8 = u8::try_from(p.leverage).unwrap_or(1);
    Some(LivePositionState {
        id,
        setup_id: p.setup_id,
        mode,
        exchange: p.exchange.clone(),
        segment,
        symbol: p.symbol.clone(),
        side,
        leverage,
        entry_avg: p.entry_avg,
        qty_filled: p.qty_filled,
        qty_remaining: p.qty_remaining,
        current_sl: p.current_sl,
        tp_ladder,
        liquidation_price: p.liquidation_price,
        maint_margin_ratio: p.maint_margin_ratio,
        funding_rate_next: None,
        last_mark: p.last_mark,
        last_tick_at: None,
        opened_at: Utc::now(),
    })
}

async fn build_live_position_for_mode(
    pool: &PgPool,
    row: &SelectedCandidateRow,
    mode: &'static str,
) -> Option<InsertLivePosition> {
    // Per-mode identity keys: dry deploys use the system shadow user,
    // live deploys use the real operator's UUID (separate key so ops can
    // flip live to a sub-account without touching the paper trail).
    let (org_key, org_env, user_key, user_env) = match mode {
        "live" => (
            "live.default_org_id",
            "QTSS_LIVE_ORG_ID",
            "live.default_user_id",
            "QTSS_LIVE_USER_ID",
        ),
        _ => (
            "dry.default_org_id",
            "QTSS_DRY_ORG_ID",
            "dry.default_user_id",
            "QTSS_DRY_USER_ID",
        ),
    };
    let org_raw = resolve_system_string(pool, MODULE, org_key, org_env, "").await;
    let user_raw = resolve_system_string(pool, MODULE, user_key, user_env, "").await;
    let org_id = Uuid::parse_str(org_raw.trim()).ok()?;
    let user_id = Uuid::parse_str(user_raw.trim()).ok()?;

    let default_segment = resolve_system_string(
        pool, MODULE, "dry.default_segment", "QTSS_DRY_SEGMENT", "futures",
    )
    .await;
    let segment = segment_from_row(&row.selector_meta, &default_segment);

    let leverage_u = resolve_system_u64(
        pool, MODULE, "dry.default_leverage", "QTSS_DRY_LEVERAGE", 10, 1, 125,
    )
    .await;
    let leverage = i16::try_from(leverage_u).unwrap_or(10);

    // Maint margin ratio — read as string (config_tick has no f64
    // helper for arbitrary precision decimals; parse the JSON-ish
    // string). Default 0.005 (50 bps) — Binance USDT-M majors.
    let mmr_raw = resolve_system_string(
        pool, MODULE, "dry.maint_margin_ratio", "QTSS_DRY_MMR", "0.005",
    )
    .await;
    let mmr = rust_decimal::Decimal::from_str_exact(mmr_raw.trim())
        .ok()
        .unwrap_or_else(|| rust_decimal::Decimal::new(5, 3));

    let side = live_side(&row.direction);
    let liq_price = liquidation_price(&segment, side, row.entry_price, leverage, mmr);

    // Backlog #4 — TP-proximity gate. User: "işlem açarken tp ile anlık
    // değerler kontrol edilemlidir. eğer tp - anlık fiyat x bir oranı
    // geçmiş ise işlem açılmasın." Concrete repro: BTCUSDT 15m IQ-T
    // armed with entry=\$77168, TP=\$77600 (only +\$432 target). By the
    // time the executor saw the row, spot was \$77536 — only \$63
    // remaining vs \$432 total target = 14% left. Position fills
    // pretty much at the TP, distorting reports as "+0.48% in profit"
    // even though the structural move is over.
    //
    // Compute remaining_to_tp_fraction = |tp − current| / |tp − entry|
    // and skip if it's below `min_tp_remaining_fraction` (default 0.5
    // — "at least half the target distance must still be available").
    let tp_price_opt = row
        .tp_ladder
        .as_array()
        .and_then(|a| a.first())
        .and_then(|t| t.get("price"))
        .and_then(|p| {
            p.as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| p.as_f64())
        });
    if let Some(tp_price) = tp_price_opt {
        // Current price = last close of the same series. Use
        // market_bars_open first (live tick) then latest market_bars
        // row as fallback.
        let current_price_opt: Option<f64> =
            current_mark_for(pool, &row.exchange, &segment, &row.symbol, &row.timeframe)
                .await;
        if let Some(current_price) = current_price_opt {
            let entry = row
                .entry_price
                .to_string()
                .parse::<f64>()
                .unwrap_or(0.0);
            let total_distance = (tp_price - entry).abs();
            let remaining = (tp_price - current_price).abs();
            if total_distance > 0.0 {
                let remaining_fraction = remaining / total_distance;
                let min_remaining = resolve_system_f64(
                    pool,
                    MODULE,
                    "min_tp_remaining_fraction",
                    "QTSS_EXEC_MIN_TP_REMAINING",
                    0.5,
                )
                .await
                .clamp(0.0, 1.0);
                if remaining_fraction < min_remaining {
                    info!(
                        symbol = %row.symbol,
                        tp = %tp_price,
                        current = %current_price,
                        remaining_pct = %(remaining_fraction * 100.0),
                        min_pct = %(min_remaining * 100.0),
                        "skip: TP-proximity gate (move already nearly done)",
                    );
                    return None;
                }
            }
        }
    }

    // Per-symbol-direction dedup — User: "$338K open notional / 3000%
    // utilisation." Part of that was the SAME (symbol, direction)
    // opening multiple times because the execution bridge polls every
    // 2s and the selected_candidates queue can serve adjacent setups
    // for the same trade. If a position already exists with this
    // (symbol, side, mode) and isn't closed, skip the new one — the
    // existing one already owns the exposure.
    let already_open = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*)::bigint FROM live_positions
            WHERE mode = $1 AND symbol = $2 AND side = $3
              AND closed_at IS NULL"#,
    )
    .bind(mode)
    .bind(&row.symbol)
    .bind(side)
    .fetch_one(pool)
    .await
    .ok()
    .unwrap_or(0);
    if already_open > 0 {
        debug!(
            symbol = %row.symbol, side = %side,
            "skip: position already open in same direction",
        );
        return None;
    }

    // Aggregate notional cap — total open notional must stay below
    // equity × leverage × aggregate_fraction. Without this an
    // operator with $1K equity could see 6 simultaneous $10K
    // positions = $60K aggregate exposure (= 60× leverage on
    // realised equity).
    let agg_fraction = resolve_system_f64(
        pool,
        MODULE,
        "aggregate_notional_fraction",
        "QTSS_EXEC_AGG_NOTIONAL_FRACTION",
        1.0,
    )
    .await
    .clamp(0.1, 5.0);
    let equity_cap_f64 = resolve_account_equity_usd(pool, MODULE, match mode {
        "live" => "live.default_equity",
        _ => "dry.default_equity",
    })
    .await;
    let max_aggregate_notional = equity_cap_f64 * (leverage as f64) * agg_fraction;
    let current_open_notional = sqlx::query_scalar::<_, Option<rust_decimal::Decimal>>(
        r#"SELECT SUM(qty_filled * entry_avg)
             FROM live_positions
            WHERE mode = $1 AND closed_at IS NULL"#,
    )
    .bind(mode)
    .fetch_one(pool)
    .await
    .ok()
    .flatten()
    .and_then(|d| d.try_into().ok())
    .unwrap_or(0.0);
    if current_open_notional >= max_aggregate_notional {
        warn!(
            mode = %mode, symbol = %row.symbol,
            current_notional = %current_open_notional,
            max_notional = %max_aggregate_notional,
            "skip: aggregate notional cap reached",
        );
        return None;
    }

    let qty = size_from_risk(pool, mode, row).await;
    // v1.1.7 — slippage + spread simulation on dry paper fills so the
    // paper-vs-live gap ChatGPT and Gemini both flagged stops being a
    // free-lunch fantasy. The adjustment nudges entry away from the
    // operator's advantage by a configurable number of basis points
    // (default 2 bps ≈ 0.02%, similar to a realistic taker + half-
    // spread on Binance USDT perps). Live mode skips the nudge — the
    // real exchange already provides its own slippage.
    let slip_bps = if mode == "dry" {
        dry_slip_bps_from_config(pool).await
    } else {
        0.0
    };
    let mut entry_adj = row.entry_price;
    if slip_bps > 0.0 {
        use rust_decimal::Decimal;
        // Long buys at a worse (higher) price; short sells at a worse
        // (lower) price. Scale = bps / 10_000.
        let factor = slip_bps / 10_000.0;
        let factor_dec =
            Decimal::try_from(factor).unwrap_or_else(|_| Decimal::new(0, 0));
        match side {
            "BUY" => entry_adj = row.entry_price + (row.entry_price * factor_dec),
            "SELL" => entry_adj = row.entry_price - (row.entry_price * factor_dec),
            _ => {}
        }
    }

    Some(InsertLivePosition {
        org_id,
        user_id,
        setup_id: Some(row.setup_id),
        mode,
        exchange: row.exchange.clone(),
        segment,
        symbol: row.symbol.clone(),
        side,
        leverage,
        entry_avg: entry_adj,
        qty_filled: qty,
        qty_remaining: qty,
        current_sl: Some(row.sl_price),
        tp_ladder: row.tp_ladder.clone(),
        liquidation_price: liq_price,
        maint_margin_ratio: Some(mmr),
        last_mark: Some(entry_adj),
        metadata: serde_json::json!({
            "selected_candidate_id": row.id,
            "setup_id": row.setup_id.to_string(),
            "timeframe": row.timeframe,
            "risk_pct": row.risk_pct.to_string(),
            "leverage": leverage,
            "slip_bps_applied": slip_bps,
            "entry_setup": row.entry_price.to_string(),
        }),
    })
}

async fn dry_slip_bps_from_config(pool: &PgPool) -> f64 {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module = 'execution'
             AND config_key = 'dry.slippage_bps'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 2.0; };
    let val: serde_json::Value = row.try_get("value").unwrap_or(serde_json::Value::Null);
    match &val {
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(2.0),
        other => other
            .get("value")
            .and_then(|v| v.as_f64())
            .unwrap_or(2.0),
    }
}

/// Faz 9.8.18 — risk-per-trade sizing.
///
///   qty = (equity * risk_pct) / |entry - sl|
///
/// Equity comes from `execution.{mode}.default_equity`; `risk_pct` is
/// the fraction already attached to the selected candidate. Leverage is
/// deliberately *not* multiplied in here — on USDT-M linear contracts
/// the notional is `qty * entry`, and the guard worker uses leverage
/// only for the liquidation formula, not for sizing. Margin = notional
/// / leverage falls out naturally.
///
/// Falls back to `0.01` if either the distance is zero/negative or the
/// equity config parses as zero, so the pipeline degrades gracefully
/// rather than hard-failing on a single bad row.
async fn size_from_risk(
    pool: &PgPool,
    mode: &'static str,
    row: &SelectedCandidateRow,
) -> rust_decimal::Decimal {
    let fallback = rust_decimal::Decimal::new(1, 2); // 0.01

    // 2026-04-26 BUG: User reported $1000 starting capital producing
    // $338K open notional / 3000% utilisation. Root cause: this
    // function used to read its OWN execution.{mode}.default_equity
    // key with hardcoded $10K dry default — completely independent of
    // the `account.equity_usd` master we wired into the rest of the
    // system. Now reads through the canonical resolver:
    //   master `account.equity_usd` first → legacy
    //   `execution.{mode}.default_equity` fallback → built-in $1K.
    let legacy_key = match mode {
        "live" => "live.default_equity",
        _ => "dry.default_equity",
    };
    let equity_f64 = resolve_account_equity_usd(pool, MODULE, legacy_key).await;
    let equity = rust_decimal::Decimal::try_from(equity_f64)
        .unwrap_or_else(|_| rust_decimal::Decimal::new(1_000, 0));
    if equity <= rust_decimal::Decimal::ZERO {
        return fallback;
    }
    let distance = (row.entry_price - row.sl_price).abs();
    if distance <= rust_decimal::Decimal::ZERO {
        return fallback;
    }
    let risk_usdt = equity * row.risk_pct;
    let raw_qty = risk_usdt / distance;

    // Notional CAP: even if risk_pct sizing implies a huge qty (tight
    // SL, narrow distance), we cap at `equity × leverage ×
    // max_position_fraction`. Without this cap a 0.1%-tight SL on
    // BTCUSDT can generate $145K notional on $1K equity — exactly
    // what the user just witnessed. Default fraction 1.0 (full
    // leverage); operators tighten via system_config.execution.
    // max_position_fraction.
    let leverage_u = resolve_system_u64(
        pool, MODULE, "dry.default_leverage", "QTSS_DRY_LEVERAGE",
        10, 1, 125,
    )
    .await;
    let leverage = rust_decimal::Decimal::from(leverage_u);
    let max_position_fraction = resolve_system_f64(
        pool,
        MODULE,
        "max_position_fraction",
        "QTSS_EXEC_MAX_POSITION_FRACTION",
        1.0,
    )
    .await
    .clamp(0.01, 1.0);
    let frac_dec = rust_decimal::Decimal::try_from(max_position_fraction)
        .unwrap_or(rust_decimal::Decimal::ONE);
    let max_notional = equity * leverage * frac_dec;
    let entry_pos = if row.entry_price > rust_decimal::Decimal::ZERO {
        row.entry_price
    } else {
        // Defensive: shouldn't happen, but if entry is zero we can't
        // derive a notional cap safely. Fall back to risk-only sizing.
        return raw_qty.round_dp(4).max(fallback);
    };
    let max_qty_by_notional = max_notional / entry_pos;

    // Final qty = the SMALLER of the two:
    //   - risk_pct sizing (preserves intended dollar risk)
    //   - notional cap (preserves capital available to lose)
    let final_qty = raw_qty.min(max_qty_by_notional);
    let qty = final_qty.round_dp(4);
    if qty <= rust_decimal::Decimal::ZERO {
        fallback
    } else {
        qty
    }
}

/// Map `selector_meta.venue_class` → canonical market segment.
/// v2 pipeline writes 'crypto' for Binance perps today; treat that as
/// 'futures' so the liquidation guard engages. Unknown values fall
/// back to `default_segment` from config.
fn segment_from_row(meta: &serde_json::Value, default_segment: &str) -> String {
    let raw = meta
        .get("venue_class")
        .and_then(|v| v.as_str())
        .unwrap_or(default_segment)
        .to_ascii_lowercase();
    match raw.as_str() {
        "futures" | "perp" | "perpetual" | "crypto" => "futures".to_string(),
        "spot" => "spot".to_string(),
        "margin" => "margin".to_string(),
        "options" => "options".to_string(),
        _ => default_segment.to_string(),
    }
}

/// Isolated-margin liquidation price approximation.
///   long:  entry * (1 - 1/lev + mmr)
///   short: entry * (1 + 1/lev - mmr)
/// Returns `None` for spot (never liquidates) or bad leverage.
/// Latest known mark price for a series. Reads `market_bars_open`
/// (live still-forming bar) first, falls back to the most recent
/// closed bar from `market_bars`. Returns `None` only when neither
/// table has any row for the tuple — which means the gate is
/// unsafe to enforce and the caller should fall through.
async fn current_mark_for(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    timeframe: &str,
) -> Option<f64> {
    if let Ok(Some(row)) = sqlx::query_scalar::<_, rust_decimal::Decimal>(
        r#"SELECT close FROM market_bars_open
            WHERE exchange = $1 AND segment = $2
              AND symbol = $3 AND interval = $4
            LIMIT 1"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .fetch_optional(pool)
    .await
    {
        return row.to_string().parse::<f64>().ok();
    }
    sqlx::query_scalar::<_, rust_decimal::Decimal>(
        r#"SELECT close FROM market_bars
            WHERE exchange = $1 AND segment = $2
              AND symbol = $3 AND interval = $4
            ORDER BY open_time DESC
            LIMIT 1"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .and_then(|d| d.to_string().parse::<f64>().ok())
}

fn liquidation_price(
    segment: &str,
    side: &str,
    entry: rust_decimal::Decimal,
    leverage: i16,
    mmr: rust_decimal::Decimal,
) -> Option<rust_decimal::Decimal> {
    if segment == "spot" || leverage <= 0 {
        return None;
    }
    let inv_lev = rust_decimal::Decimal::ONE / rust_decimal::Decimal::from(leverage);
    let factor = match side {
        "BUY" => rust_decimal::Decimal::ONE - inv_lev + mmr,
        "SELL" => rust_decimal::Decimal::ONE + inv_lev - mmr,
        _ => return None,
    };
    Some(entry * factor)
}

fn live_side(d: &str) -> &'static str {
    match d {
        "long" => "BUY",
        _ => "SELL",
    }
}
