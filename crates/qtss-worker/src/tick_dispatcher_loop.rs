//! Faz 9.8.14 — tick dispatcher loop.
//!
//! Glues three pieces that were designed in isolation:
//!   1. `live_positions` DB rows (seeded by execution_bridge).
//!   2. `qtss-risk::LivePositionStore` + `evaluate_tick` (pure).
//!   3. `qtss-notify::PriceTickStore` (populated by bookTicker WS).
//!
//! On startup we hydrate the store from the DB. Then a tight loop polls
//! PriceTickStore for every registered `TickKey`, calls `evaluate_tick`,
//! and persists actionable outcomes:
//!   * liquidation severity != Safe  → liquidation_guard_events
//!   * scale decision != Hold        → position_scale_events
//!   * ratchet kind != None          → position_scale_events (ratchet_sl)
//!   * tp triggers                   → position_scale_events (partial_tp)
//! Any mark we processed is flushed to `live_positions.last_mark` so the
//! GUI reflects current state.
//!
//! CLAUDE.md #1 — persistence is a dispatch-by-match over outcome kinds,
//! each branch is a one-liner delegating to a helper.
//! CLAUDE.md #2 — every knob (enabled/interval/stale/hydrate) comes
//! from `system_config` under `risk.tick_dispatcher.*`.
//! CLAUDE.md #3 — storage stays DTO-shaped; translation lives here.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use qtss_notify::PriceTickStore;
use qtss_risk::{
    evaluate_tick, ExecutionMode, LivePositionState, LivePositionStore, MarketSegment,
    PositionSide, PositionTickOutcomes, RatchetKind, ScaleDecisionKind, TickContext,
    TickDispatcherConfig, TpLeg,
};
use qtss_storage::{
    insert_liquidation_guard_event, insert_position_scale_event, list_open_live_positions,
    resolve_system_u64, resolve_worker_enabled_flag, update_live_position_mark,
    InsertLiquidationEvent, InsertScaleEvent, LivePositionRow,
};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use tracing::{info, warn};

const MODULE: &str = "risk";
const CFG_ENABLED: &str = "tick_dispatcher.enabled";
const CFG_EVAL_MS: &str = "tick_dispatcher.eval_interval_ms";
const CFG_HYDRATE_SECS: &str = "tick_dispatcher.hydrate_interval_secs";
const CFG_STALE_SECS: &str = "tick_dispatcher.stale_mark_secs";
const ENV_ENABLED: &str = "QTSS_TICK_DISPATCHER_ENABLED";
const ENV_EVAL_MS: &str = "QTSS_TICK_DISPATCHER_EVAL_MS";
const ENV_HYDRATE_SECS: &str = "QTSS_TICK_DISPATCHER_HYDRATE_SECS";
const ENV_STALE_SECS: &str = "QTSS_TICK_DISPATCHER_STALE_SECS";

pub async fn tick_dispatcher_loop(
    pool: PgPool,
    store: Arc<LivePositionStore>,
    price_store: Arc<PriceTickStore>,
) {
    info!("tick dispatcher loop: starting");
    // Hydrate up-front so the loop begins with a warm store.
    hydrate(&pool, &store).await;

    let mut last_hydrate = Utc::now();
    loop {
        let enabled =
            resolve_worker_enabled_flag(&pool, MODULE, CFG_ENABLED, ENV_ENABLED, true).await;
        let eval_ms =
            resolve_system_u64(&pool, MODULE, CFG_EVAL_MS, ENV_EVAL_MS, 1_000, 200, 60_000).await;
        let hydrate_secs =
            resolve_system_u64(&pool, MODULE, CFG_HYDRATE_SECS, ENV_HYDRATE_SECS, 60, 5, 3_600)
                .await;
        let stale_secs =
            resolve_system_u64(&pool, MODULE, CFG_STALE_SECS, ENV_STALE_SECS, 30, 1, 600).await;

        if !enabled {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        if (Utc::now() - last_hydrate).num_seconds() >= hydrate_secs as i64 {
            hydrate(&pool, &store).await;
            last_hydrate = Utc::now();
        }
        if let Err(e) = run_sweep(&pool, &store, &price_store, stale_secs as i64).await {
            warn!(error = %e, "tick dispatcher sweep failed");
        }
        tokio::time::sleep(Duration::from_millis(eval_ms.max(200))).await;
    }
}

async fn run_sweep(
    pool: &PgPool,
    store: &LivePositionStore,
    price_store: &PriceTickStore,
    stale_secs: i64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cfg = TickDispatcherConfig::default();
    let ctx = TickContext::default();
    let now = Utc::now();
    for key in store.tick_keys() {
        // PriceTickStore keys by exchange+symbol only; ok for v1 since
        // we only trade binance futures. Future work: segment-aware
        // store or separate markPrice stream.
        let Some(tick) = price_store.get(&key.exchange, &key.symbol) else {
            continue;
        };
        if tick.age_ms(now) > stale_secs * 1_000 {
            continue;
        }
        let mid = tick.mid();
        let outcomes = evaluate_tick(store, &key, mid, tick.received_at, &cfg, &ctx);
        for pos in outcomes.positions.iter() {
            // Always flush the mark so live_positions.last_mark is current.
            if let Err(e) = update_live_position_mark(pool, pos.position_id, mid).await {
                warn!(id = %pos.position_id, error = %e, "update_live_position_mark");
            }
            if !pos.has_action() {
                continue;
            }
            persist_outcome(pool, pos, mid, tick.received_at).await;
        }
    }
    Ok(())
}

async fn persist_outcome(
    pool: &PgPool,
    pos: &PositionTickOutcomes,
    mark: Decimal,
    at: DateTime<Utc>,
) {
    if let Some(liq) = &pos.liquidation {
        if let Some(sev) = qtss_risk::liquidation_severity_db_tag(liq.severity) {
            let evt = InsertLiquidationEvent {
                position_id: pos.position_id,
                severity: sev,
                action_taken: qtss_risk::liquidation_action_db_tag(liq.action),
                mark_price: liq.mark,
                liquidation_price: liq.liquidation,
                distance_pct: liq.distance_pct,
                margin_ratio: None,
                details: json!({ "at": at.to_rfc3339() }),
            };
            if let Err(e) = insert_liquidation_guard_event(pool, &evt).await {
                warn!(id = %pos.position_id, error = %e, "liquidation event insert");
            }
        }
    }

    let scale_kind = scale_event_kind(pos.scale.kind);
    if let Some(kind) = scale_kind {
        // Snapshot qty_after/entry_avg_after best-effort: without the
        // pre-state we don't know the delta's starting point, so we
        // record the mark as price and leave qty_after/entry_avg_after
        // zero — downstream reporting reads the follow-up position
        // row, not this column, for canonical state. Fine for 9.8.14.
        let evt = InsertScaleEvent {
            position_id: pos.position_id,
            event_kind: kind,
            price: mark,
            qty_delta: pos.scale.qty_delta,
            qty_after: Decimal::ZERO,
            entry_avg_after: Decimal::ZERO,
            realized_pnl_quote: None,
            reason: Some(pos.scale.reason.to_string()),
            metadata: json!({ "at": at.to_rfc3339() }),
        };
        if let Err(e) = insert_position_scale_event(pool, &evt).await {
            warn!(id = %pos.position_id, error = %e, "scale event insert");
        }
    }

    if let Some(new_sl) = ratchet_event(&pos) {
        let evt = InsertScaleEvent {
            position_id: pos.position_id,
            event_kind: "ratchet_sl",
            price: mark,
            qty_delta: Decimal::ZERO,
            qty_after: Decimal::ZERO,
            entry_avg_after: Decimal::ZERO,
            realized_pnl_quote: None,
            reason: Some(format!("ratchet={:?}", pos.ratchet.kind)),
            metadata: json!({ "new_sl": new_sl.to_string(), "at": at.to_rfc3339() }),
        };
        if let Err(e) = insert_position_scale_event(pool, &evt).await {
            warn!(id = %pos.position_id, error = %e, "ratchet event insert");
        }
    }

    for trig in &pos.tp_triggers {
        let evt = InsertScaleEvent {
            position_id: pos.position_id,
            event_kind: "partial_tp",
            price: trig.price,
            qty_delta: -trig.qty,
            qty_after: Decimal::ZERO,
            entry_avg_after: Decimal::ZERO,
            realized_pnl_quote: None,
            reason: Some(format!("tp_leg_{}", trig.leg_index)),
            metadata: json!({
                "leg_index": trig.leg_index,
                "at": at.to_rfc3339(),
            }),
        };
        if let Err(e) = insert_position_scale_event(pool, &evt).await {
            warn!(id = %pos.position_id, error = %e, "tp event insert");
        }
    }
}

fn scale_event_kind(k: ScaleDecisionKind) -> Option<&'static str> {
    match k {
        ScaleDecisionKind::Hold => None,
        ScaleDecisionKind::PyramidIn => Some("scale_in"),
        ScaleDecisionKind::ScaleOut => Some("scale_out"),
        ScaleDecisionKind::AddOnDip => Some("add_on_dip"),
        ScaleDecisionKind::PartialTp => Some("partial_tp"),
    }
}

fn ratchet_event(pos: &PositionTickOutcomes) -> Option<Decimal> {
    match pos.ratchet.kind {
        RatchetKind::None => None,
        _ => pos.ratchet.new_sl,
    }
}

async fn hydrate(pool: &PgPool, store: &LivePositionStore) {
    let rows = match list_open_live_positions(pool, None).await {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "hydrate: list_open_live_positions");
            return;
        }
    };
    let mut loaded = 0usize;
    for row in rows {
        let Some(state) = to_state(&row) else {
            continue;
        };
        store.upsert(state);
        loaded += 1;
    }
    info!(loaded, "tick dispatcher: hydrated store");
}

fn to_state(row: &LivePositionRow) -> Option<LivePositionState> {
    let mode = match row.mode.as_str() {
        "dry" => ExecutionMode::Dry,
        "live" => ExecutionMode::Live,
        _ => return None,
    };
    let segment = MarketSegment::parse(&row.segment)?;
    let side = match row.side.as_str() {
        "BUY" | "buy" | "long" => PositionSide::Buy,
        "SELL" | "sell" | "short" => PositionSide::Sell,
        _ => return None,
    };
    let tp_ladder: Vec<TpLeg> = serde_json::from_value(row.tp_ladder.clone()).unwrap_or_default();
    let leverage: u8 = row.leverage.try_into().unwrap_or(1);
    Some(LivePositionState {
        id: row.id,
        setup_id: row.setup_id,
        mode,
        exchange: row.exchange.clone(),
        segment,
        symbol: row.symbol.clone(),
        side,
        leverage,
        entry_avg: row.entry_avg,
        qty_filled: row.qty_filled,
        qty_remaining: row.qty_remaining,
        current_sl: row.current_sl,
        tp_ladder,
        liquidation_price: row.liquidation_price,
        maint_margin_ratio: row.maint_margin_ratio,
        funding_rate_next: row.funding_rate_next,
        last_mark: row.last_mark,
        last_tick_at: row.last_tick_at,
        opened_at: row.opened_at,
    })
}

