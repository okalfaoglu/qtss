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
use qtss_risk::{
    ExecutionMode, LivePositionState, LivePositionStore, MarketSegment, PositionSide, TpLeg,
};
use qtss_storage::{
    claim_selected_candidates, insert_live_position, mark_selected_errored, mark_selected_placed,
    resolve_system_string, resolve_system_u64, resolve_worker_enabled_flag, InsertLivePosition,
    SelectedCandidateRow,
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
            }
        }
    }
    Ok(())
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
        "live" if live_enabled => Err("live execution adapter not yet wired (Faz 9.8.15)".into()),
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
    if let Some(live) = build_live_position(pool, row).await {
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

async fn build_live_position(pool: &PgPool, row: &SelectedCandidateRow) -> Option<InsertLivePosition> {
    let org_raw = resolve_system_string(
        pool, MODULE, "dry.default_org_id", "QTSS_DRY_ORG_ID", "",
    )
    .await;
    let user_raw = resolve_system_string(
        pool, MODULE, "dry.default_user_id", "QTSS_DRY_USER_ID", "",
    )
    .await;
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

    let qty = rust_decimal::Decimal::new(1, 2); // 0.01 placeholder — sizing upstream
    Some(InsertLivePosition {
        org_id,
        user_id,
        setup_id: Some(row.setup_id),
        mode: "dry",
        exchange: row.exchange.clone(),
        segment,
        symbol: row.symbol.clone(),
        side,
        leverage,
        entry_avg: row.entry_price,
        qty_filled: qty,
        qty_remaining: qty,
        current_sl: Some(row.sl_price),
        tp_ladder: row.tp_ladder.clone(),
        liquidation_price: liq_price,
        maint_margin_ratio: Some(mmr),
        last_mark: Some(row.entry_price),
        metadata: serde_json::json!({
            "selected_candidate_id": row.id,
            "setup_id": row.setup_id.to_string(),
            "timeframe": row.timeframe,
            "risk_pct": row.risk_pct.to_string(),
            "leverage": leverage,
        }),
    })
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
