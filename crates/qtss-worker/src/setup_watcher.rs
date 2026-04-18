//! Faz 9.7.3 + 9.7.4 — SetupWatcher loop.
//!
//! Each tick:
//!   1. `list_watcher_rows()` — rich projection with ratchet state
//!   2. for each row:
//!      * build `WatcherSetupState` (current_sl may be from ratchet)
//!      * read latest [`PriceTick`] from shared store
//!      * **Poz Koruma (9.7.4)**: call `evaluate_poz_koruma`; if
//!        Ratcheted, update DB + emit `SlRatcheted`
//!      * run pure `detect_transitions`
//!      * for `TpHit`: call **Smart Target AI (9.7.4)** →
//!        map action → lifecycle event kind
//!      * build context (with AI fields filled) → router.dispatch
//!   3. compute Health Score; persist snapshot on band change only
//!
//! CLAUDE.md #1 — Smart Target action → lifecycle kind happens via a
//! tiny match on the enum, one arm each. #2 — every tunable (enable
//! flags, tick secs, EOD hour, gain threshold, rule/llm cutoffs) is
//! pulled from `system_config`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use qtss_notify::{
    compute_health, decide_on_approach, decide_smart_target, detect_transitions,
    evaluate_poz_koruma, load_approach_config, load_health_bands, load_health_weights,
    load_poz_koruma_config, load_smart_target_config, make_context, promote_tp_hit, ApproachCfg,
    DbPersistHandler, DefaultLlmJudge, HealthBand, LlmJudge, NotificationDispatcher,
    TelegramLifecycleHandler, XOutboxHandler, HealthComponents, HealthScore, LifecycleDecision,
    LifecycleEventKind, LifecycleRouter, PriceTickStore, RatchetInput, RatchetOutcome,
    SetupDirection, SmartTargetAction, SmartTargetInput, WatcherSetupState,
};
use qtss_storage::{
    apply_ratchet_update, apply_trail_advance, apply_trail_enable, insert_health_snapshot,
    list_watcher_rows, mark_ai_advised, resolve_worker_enabled_flag, resolve_worker_tick_secs,
    HealthSnapshotInsert, RatchetUpdate, WatcherSetupRow,
};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

const MODULE: &str = "notify";

#[derive(Debug, Default, Clone)]
struct SetupMemo {
    last_band: Option<HealthBand>,
    last_health_total: Option<f64>,
}

type MemoMap = Arc<Mutex<HashMap<Uuid, SetupMemo>>>;

fn to_decimal_f32(f: Option<f32>) -> Option<Decimal> {
    f.and_then(|v| Decimal::from_f32_retain(v))
}

fn parse_direction(s: &str) -> Option<SetupDirection> {
    match s.to_ascii_lowercase().as_str() {
        "long" => Some(SetupDirection::Long),
        "short" => Some(SetupDirection::Short),
        _ => None,
    }
}

fn tp_from_meta(meta: &serde_json::Value, key: &str) -> Option<Decimal> {
    meta.get(key).and_then(|v| v.as_f64()).and_then(Decimal::from_f64)
}

fn build_state(row: &WatcherSetupRow) -> Option<WatcherSetupState> {
    let direction = parse_direction(&row.direction)?;
    let entry = to_decimal_f32(row.entry_price)?;
    // Ratchet-aware SL precedence: `current_sl` (ratcheted) > `koruma`
    // (initial protection) > `entry_sl` (fallback).
    let sl = row
        .current_sl
        .or_else(|| to_decimal_f32(row.koruma))
        .or_else(|| to_decimal_f32(row.entry_sl))?;
    let tp_prices = [
        tp_from_meta(&row.raw_meta, "tp1_price"),
        tp_from_meta(&row.raw_meta, "tp2_price"),
        tp_from_meta(&row.raw_meta, "tp3_price"),
    ];
    let bitmap = (row.tp_hits_bitmap.max(0) & 0xFF) as u8;
    Some(WatcherSetupState {
        setup_id: row.id,
        exchange: row.exchange.clone(),
        symbol: row.symbol.clone(),
        direction,
        entry_price: entry,
        current_sl: sl,
        tp_prices,
        tp_hits_bitmap: bitmap,
        entry_touched: row.entry_touched_at.is_some(),
        opened_at: row.created_at,
    })
}

fn total_tps(state: &WatcherSetupState) -> u8 {
    state.tp_prices.iter().filter(|t| t.is_some()).count() as u8
}

fn price_progress_components(state: &WatcherSetupState, price: Decimal) -> HealthComponents {
    let entry = state.entry_price.to_f64().unwrap_or(0.0);
    let px = price.to_f64().unwrap_or(0.0);
    let sl = state.current_sl.to_f64().unwrap_or(0.0);
    let target = state.tp_prices.iter().flatten().last().and_then(|d| d.to_f64());
    let sl_dist = match state.direction {
        SetupDirection::Long => ((px - sl) / ((entry - sl).max(1e-9))).clamp(0.0, 1.5),
        SetupDirection::Short => ((sl - px) / ((sl - entry).max(1e-9))).clamp(0.0, 1.5),
    };
    let sl_score = (sl_dist / 1.5 * 100.0).clamp(0.0, 100.0);
    let tp_score = target.map(|t| match state.direction {
        SetupDirection::Long => ((px - entry) / (t - entry).max(1e-9) * 100.0).clamp(0.0, 100.0),
        SetupDirection::Short => ((entry - px) / (entry - t).max(1e-9) * 100.0).clamp(0.0, 100.0),
    });
    let momentum = match tp_score {
        Some(tp) => Some(0.6 * sl_score + 0.4 * tp),
        None => Some(sl_score),
    };
    HealthComponents { momentum, ..Default::default() }
}

/// Small dispatch table: Smart Target action → lifecycle event kind.
/// Tighten emits `SlRatcheted` (it's an SL move, not a TP execution).
fn action_to_kind(action: SmartTargetAction) -> LifecycleEventKind {
    match action {
        SmartTargetAction::Ride => LifecycleEventKind::TpHit,
        SmartTargetAction::Scale => LifecycleEventKind::TpPartial,
        SmartTargetAction::Exit => LifecycleEventKind::TpFinal,
        // Trail = new SL regime; reuse the ratchet event kind.
        SmartTargetAction::Tighten | SmartTargetAction::Trail => LifecycleEventKind::SlRatcheted,
    }
}

/// Returns Some(tp_idx) (1-based) if price is within `approach_pct`
/// of an *unhit* TP on the favourable side, else None. Scans TPs in
/// order so the closest un-touched target wins.
fn approaching_tp(
    state: &WatcherSetupState,
    price: Decimal,
    approach_pct: f64,
) -> Option<u8> {
    let px = price.to_f64().unwrap_or(0.0);
    for (i, tp) in state.tp_prices.iter().enumerate() {
        let Some(tp_price) = tp else { continue };
        let idx = (i as u8) + 1;
        let bit = 1u8 << (idx - 1);
        if state.tp_hits_bitmap & bit != 0 {
            continue; // already hit
        }
        let tp_f = tp_price.to_f64().unwrap_or(0.0);
        if tp_f <= 0.0 {
            continue;
        }
        let dist = (tp_f - px).abs() / tp_f;
        if dist > approach_pct {
            continue;
        }
        // Must still be below (long) / above (short) the TP — once
        // crossed, the TpHit path owns the decision.
        let on_approach_side = match state.direction {
            SetupDirection::Long => px < tp_f,
            SetupDirection::Short => px > tp_f,
        };
        if on_approach_side {
            return Some(idx);
        }
    }
    None
}

/// Compute the initial / advancing trail SL from an anchor price.
/// buffer_pct is a fraction of the anchor (e.g. 0.008 → 0.8%).
fn trail_sl_from_anchor(
    direction: SetupDirection,
    anchor: Decimal,
    buffer_pct: f64,
) -> Decimal {
    let a = anchor.to_f64().unwrap_or(0.0);
    let sl = match direction {
        SetupDirection::Long => a * (1.0 - buffer_pct),
        SetupDirection::Short => a * (1.0 + buffer_pct),
    };
    Decimal::from_f64(sl).unwrap_or(anchor)
}

#[allow(clippy::too_many_arguments)]
async fn handle_tp_approach(
    pool: &PgPool,
    router: &LifecycleRouter,
    row: &WatcherSetupRow,
    state: &WatcherSetupState,
    price: Decimal,
    tp_idx: u8,
    health: HealthScore,
    prev_band: Option<HealthBand>,
    smart_cfg: &qtss_notify::SmartTargetCfg,
    approach_cfg: &ApproachCfg,
    llm: &dyn LlmJudge,
) {
    // Debounce: one AI call per TP level per setup.
    if row.ai_advised_tp_idx.map(|v| v as u8) >= Some(tp_idx) {
        return;
    }
    let s_input = SmartTargetInput {
        tp_index: tp_idx,
        total_tps: total_tps(state),
        health,
        price,
        pnl_pct: None,
    };
    let (decision, kind_used) =
        decide_on_approach(&s_input, smart_cfg, approach_cfg, llm).await;
    let now = Utc::now();
    debug!(
        setup_id=%state.setup_id, tp=tp_idx,
        action=%decision.action.code(), evaluator=?kind_used,
        "tp_approach decision"
    );

    match decision.action {
        SmartTargetAction::Trail => {
            let new_sl = trail_sl_from_anchor(state.direction, price, approach_cfg.trail_buffer_pct);
            // Only enable if the new SL is actually better than current.
            let improves = match state.direction {
                SetupDirection::Long => new_sl > state.current_sl,
                SetupDirection::Short => new_sl < state.current_sl,
            };
            if !improves {
                // Treat as a no-op; still mark the TP as advised so we
                // don't loop on the AI call.
                if let Err(e) = mark_ai_advised(pool, row.id, tp_idx as i16, now).await {
                    warn!(%e, setup_id=%row.id, "mark_ai_advised");
                }
                return;
            }
            if let Err(e) = apply_trail_enable(pool, row.id, price, new_sl, tp_idx as i16, now).await
            {
                warn!(%e, setup_id=%row.id, "apply_trail_enable");
                return;
            }
            // Emit SlRatcheted with AI reasoning so GUI/TG see the switch.
            let lc = LifecycleDecision {
                kind: LifecycleEventKind::SlRatcheted,
                tp_index: Some(tp_idx),
                price: new_sl,
            };
            let mut ctx = make_context(state, &lc, Some(health), prev_band, now);
            ctx.ai_action = Some(decision.action.code().to_string());
            ctx.ai_reasoning = Some(format!("[trail] {}", decision.reasoning));
            ctx.ai_confidence = Some(decision.confidence);
            router.dispatch(&ctx).await;
        }
        _ => {
            // Non-trail approach decisions are advisory only at this
            // stage — we record them and let the actual TpHit flow
            // (handle_tp_hit) execute the action on touch. This keeps
            // the existing semantics intact for Ride/Scale/Exit/Tighten.
            if let Err(e) = mark_ai_advised(pool, row.id, tp_idx as i16, now).await {
                warn!(%e, setup_id=%row.id, "mark_ai_advised");
            }
        }
    }
}

/// Advance the trailing stop if price makes a new favourable extreme.
/// Monotonic: SL only tightens. Emits no event on each advance (too
/// chatty) — existing SlHit detection will fire when price touches the
/// ratcheted SL.
async fn run_trail_advance(
    pool: &PgPool,
    row: &WatcherSetupRow,
    state: &WatcherSetupState,
    price: Decimal,
    approach_cfg: &ApproachCfg,
) {
    if !row.trail_mode {
        return;
    }
    let Some(prev_anchor) = row.trail_anchor else { return };
    let improves_anchor = match state.direction {
        SetupDirection::Long => price > prev_anchor,
        SetupDirection::Short => price < prev_anchor,
    };
    if !improves_anchor {
        return;
    }
    let new_sl = trail_sl_from_anchor(state.direction, price, approach_cfg.trail_buffer_pct);
    let tighter = match state.direction {
        SetupDirection::Long => new_sl > state.current_sl,
        SetupDirection::Short => new_sl < state.current_sl,
    };
    if !tighter {
        // Anchor advanced but buffer still leaves SL unchanged —
        // persist anchor only (no SL move) to keep state coherent.
        if let Err(e) = apply_trail_advance(pool, row.id, price, state.current_sl).await {
            warn!(%e, setup_id=%row.id, "trail anchor-only advance");
        }
        return;
    }
    if let Err(e) = apply_trail_advance(pool, row.id, price, new_sl).await {
        warn!(%e, setup_id=%row.id, "apply_trail_advance");
    }
}

async fn run_poz_koruma(
    pool: &PgPool,
    router: &LifecycleRouter,
    row: &WatcherSetupRow,
    state: &WatcherSetupState,
    tick_mid: Decimal,
    cfg: &qtss_notify::PozKorumaConfig,
    health: &HealthScore,
) {
    let original_sl = to_decimal_f32(row.entry_sl)
        .or_else(|| to_decimal_f32(row.koruma))
        .unwrap_or(state.current_sl);
    let cumulative_pct = row
        .ratchet_cumulative_pct
        .as_ref()
        .and_then(|d| d.to_f64())
        .unwrap_or(0.0);
    let input = RatchetInput {
        direction: state.direction,
        entry_price: state.entry_price,
        current_price: tick_mid,
        original_sl,
        current_sl: state.current_sl,
        reference_price: row.ratchet_reference_price,
        cumulative_pct,
        last_update_at: row.ratchet_last_update_at,
    };
    let outcome = evaluate_poz_koruma(&input, cfg, Utc::now());
    match outcome {
        RatchetOutcome::NoChange => {}
        RatchetOutcome::ReferenceOnly { new_reference_price, at, .. } => {
            let u = RatchetUpdate {
                setup_id: row.id,
                current_sl: None,
                ratchet_reference_price: new_reference_price,
                ratchet_cumulative_pct: cumulative_pct,
                ratchet_last_update_at: at,
            };
            if let Err(e) = apply_ratchet_update(pool, &u).await {
                warn!(%e, setup_id=%row.id, "poz_koruma: ref-only apply");
            }
        }
        RatchetOutcome::Ratcheted(step) => {
            let u = RatchetUpdate {
                setup_id: row.id,
                current_sl: Some(step.new_sl),
                ratchet_reference_price: step.new_reference_price,
                ratchet_cumulative_pct: step.new_cumulative_pct,
                ratchet_last_update_at: step.at,
            };
            if let Err(e) = apply_ratchet_update(pool, &u).await {
                warn!(%e, setup_id=%row.id, "poz_koruma: ratchet apply");
                return;
            }
            // Emit lifecycle event so downstream channels see the move.
            let decision = LifecycleDecision {
                kind: LifecycleEventKind::SlRatcheted,
                tp_index: None,
                price: step.new_sl,
            };
            let mut ctx = make_context(state, &decision, Some(*health), None, step.at);
            ctx.ai_action = None;
            ctx.ai_reasoning = Some(format!(
                "Poz Koruma: günlük +{:.2}% kilitlendi (toplam {:.2}%)",
                step.gained_pct, step.new_cumulative_pct
            ));
            router.dispatch(&ctx).await;
        }
    }
}

async fn handle_tp_hit(
    router: &LifecycleRouter,
    state: &WatcherSetupState,
    decision: LifecycleDecision,
    health: HealthScore,
    prev_band: Option<HealthBand>,
    smart_cfg: &qtss_notify::SmartTargetCfg,
    llm: &dyn LlmJudge,
) {
    let Some(idx) = decision.tp_index else { return };
    let s_input = SmartTargetInput {
        tp_index: idx,
        total_tps: total_tps(state),
        health,
        price: decision.price,
        pnl_pct: None,
    };
    let (st_decision, kind_used) = decide_smart_target(&s_input, smart_cfg, llm).await;
    let final_kind = action_to_kind(st_decision.action);

    let final_decision = LifecycleDecision { kind: final_kind, ..decision };
    let mut ctx = make_context(state, &final_decision, Some(health), prev_band, Utc::now());
    ctx.ai_action = Some(st_decision.action.code().to_string());
    ctx.ai_reasoning = Some(st_decision.reasoning);
    ctx.ai_confidence = Some(st_decision.confidence);
    debug!(
        setup_id=%state.setup_id,
        tp=idx,
        action=%st_decision.action.code(),
        evaluator=?kind_used,
        "smart_target decision"
    );
    router.dispatch(&ctx).await;
}

async fn maybe_persist_health(
    pool: &PgPool,
    memos: &MemoMap,
    setup_id: Uuid,
    health: &HealthScore,
    price: Decimal,
    band_delta_min: u8,
) {
    let (prev_band, prev_total) = {
        let g = memos.lock().await;
        let m = g.get(&setup_id).cloned().unwrap_or_default();
        (m.last_band, m.last_health_total)
    };
    let band_changed = match prev_band {
        None => true,
        Some(pb) => {
            let delta = (pb.index() as i16 - health.band.index() as i16).unsigned_abs() as u8;
            delta >= band_delta_min
        }
    };
    if !band_changed {
        return;
    }
    let snap = HealthSnapshotInsert {
        setup_id,
        health_score: health.total,
        prev_health_score: prev_total,
        band: health.band.code().to_string(),
        prev_band: prev_band.map(|b| b.code().to_string()),
        momentum_score: health.components.momentum,
        structural_score: health.components.structural,
        orderbook_score: health.components.orderbook,
        regime_match_score: health.components.regime,
        correlation_score: health.components.correlation,
        ai_rescore: health.components.ai_rescore,
        price,
    };
    if let Err(e) = insert_health_snapshot(pool, &snap).await {
        warn!(%e, %setup_id, "insert_health_snapshot");
        return;
    }
    let mut g = memos.lock().await;
    let m = g.entry(setup_id).or_default();
    m.last_band = Some(health.band);
    m.last_health_total = Some(health.total);
}

#[allow(clippy::too_many_arguments)]
async fn tick_once(
    pool: &PgPool,
    store: &PriceTickStore,
    memos: &MemoMap,
    router: &LifecycleRouter,
    weights: &qtss_notify::HealthWeights,
    bands: &qtss_notify::HealthBands,
    poz_cfg: &qtss_notify::PozKorumaConfig,
    smart_cfg: &qtss_notify::SmartTargetCfg,
    approach_cfg: &ApproachCfg,
    llm: &dyn LlmJudge,
    band_delta_min: u8,
) -> Result<usize, qtss_storage::StorageError> {
    let rows = list_watcher_rows(pool).await?;
    let mut processed = 0_usize;
    for row in rows.iter() {
        let Some(state) = build_state(row) else {
            debug!(setup_id=%row.id, "watcher skip (missing fields)");
            continue;
        };
        let Some(tick) = store.get(&state.exchange, &state.symbol) else {
            continue;
        };
        let mid = tick.mid();

        // --- Health (every tick) ------------------------------------
        let components = price_progress_components(&state, mid);
        let health = compute_health(weights, &components, bands);
        let prev_band = {
            let g = memos.lock().await;
            g.get(&row.id).and_then(|m| m.last_band)
        };

        // --- Trail advance (9.7.5): if trail_mode on, tighten SL on new HH/LL
        // before poz_koruma so the two ratchets don't fight. Trail is the
        // stronger signal once AI has enabled it.
        run_trail_advance(pool, row, &state, mid, approach_cfg).await;

        // --- Poz Koruma --------------------------------------------
        run_poz_koruma(pool, router, row, &state, mid, poz_cfg, &health).await;

        // --- TP-approach AI advisory (9.7.5) -----------------------
        // Fires once per unhit TP when price enters the approach band.
        // May flip the setup into trail_mode (→ SL tightens via trail).
        if approach_cfg.enabled && !row.trail_mode {
            if let Some(tp_idx) = approaching_tp(&state, mid, approach_cfg.approach_pct) {
                handle_tp_approach(
                    pool, router, row, &state, mid, tp_idx, health, prev_band,
                    smart_cfg, approach_cfg, llm,
                )
                .await;
            }
        }

        // --- Transitions + Smart Target ----------------------------
        let decisions = detect_transitions(&state, &tick);
        for decision in decisions {
            match decision.kind {
                LifecycleEventKind::TpHit => {
                    handle_tp_hit(router, &state, decision, health, prev_band, smart_cfg, llm).await;
                }
                _ => {
                    // Non-TP transitions fall back to 9.7.3 promotion.
                    let mut promoted = promote_tp_hit(&state, decision);
                    // Distinguish trail_stop from plain sl_hit (telemetry).
                    if promoted.kind == LifecycleEventKind::SlHit && row.trail_mode {
                        promoted.kind = LifecycleEventKind::TrailStop;
                    }
                    let ctx = make_context(&state, &promoted, Some(health), prev_band, Utc::now());
                    router.dispatch(&ctx).await;
                }
            }
        }

        // --- Persist health snapshot on band change ---------------
        maybe_persist_health(pool, memos, row.id, &health, mid, band_delta_min).await;

        processed += 1;
    }
    Ok(processed)
}

pub async fn setup_watcher_loop(pool: PgPool, store: Arc<PriceTickStore>) {
    info!("setup_watcher loop spawned");
    let memos: MemoMap = Arc::new(Mutex::new(HashMap::new()));
    // Build handler dispatch table. Telegram handler is additive —
    // absent config just no-ops inside the handler (dispatcher check).
    let tg_enabled = resolve_worker_enabled_flag(
        &pool,
        MODULE,
        "telegram_lifecycle.enabled",
        "QTSS_NOTIFY_TG_LIFECYCLE_ENABLED",
        true,
    )
    .await;
    let mut handlers: Vec<Arc<dyn qtss_notify::LifecycleHandler>> =
        vec![Arc::new(DbPersistHandler::new(pool.clone()))];
    if tg_enabled {
        let dispatcher = NotificationDispatcher::from_env();
        handlers.push(Arc::new(TelegramLifecycleHandler::new(dispatcher)));
    }
    let x_enabled = resolve_worker_enabled_flag(
        &pool,
        MODULE,
        "x_outbox_handler.enabled",
        "QTSS_NOTIFY_X_OUTBOX_HANDLER_ENABLED",
        true,
    )
    .await;
    if x_enabled {
        handlers.push(Arc::new(XOutboxHandler::new(pool.clone())));
    }
    let router = LifecycleRouter::new(handlers);
    // Prefer a real LLM-backed judge; fall back to the deterministic
    // stub when the AI engine is disabled / misconfigured so the
    // watcher keeps running (Faz 9.7.5 task b).
    let llm: Arc<dyn LlmJudge> =
        match qtss_ai::SmartTargetLlmJudge::try_build(&pool).await {
            Some(j) => {
                info!(judge = j.name(), "setup_watcher: LLM judge active");
                j
            }
            None => {
                info!("setup_watcher: LLM judge disabled → DefaultLlmJudge stub");
                Arc::new(DefaultLlmJudge)
            }
        };
    info!(handlers = ?router.handler_names(), "setup_watcher router ready");

    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            MODULE,
            "setup_watcher.enabled",
            "QTSS_NOTIFY_SETUP_WATCHER_ENABLED",
            false,
        )
        .await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        let tick_secs = resolve_worker_tick_secs(
            &pool,
            MODULE,
            "setup_watcher.tick_secs",
            "QTSS_NOTIFY_SETUP_WATCHER_TICK_SECS",
            5,
            1,
        )
        .await;
        let band_delta_min = qtss_storage::resolve_system_u64(
            &pool,
            MODULE,
            "price_watcher.health_persist_min_band_delta",
            "QTSS_NOTIFY_HEALTH_BAND_DELTA_MIN",
            1,
            1,
            3,
        )
        .await as u8;
        let weights = load_health_weights(&pool).await;
        let bands = load_health_bands(&pool).await;
        let poz_cfg = load_poz_koruma_config(&pool).await;
        let smart_cfg = load_smart_target_config(&pool).await;
        let approach_cfg = load_approach_config(&pool).await;

        match tick_once(
            &pool, &store, &memos, &router, &weights, &bands, &poz_cfg, &smart_cfg,
            &approach_cfg, llm.as_ref(), band_delta_min,
        )
        .await
        {
            Ok(n) => debug!(processed = n, "setup_watcher tick"),
            Err(e) => warn!(%e, "setup_watcher tick"),
        }
        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}
