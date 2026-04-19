//! Faz 8.0 — Setup Engine worker loop.
//!
//! Per tick (default 30s), for each enabled engine symbol:
//!   1. Update existing open setups for this (venue, symbol, timeframe,
//!      profile): ratchet + stop/target/reverse close checks.
//!   2. If no open setup exists for the derived profile and confluence
//!      is strong enough, evaluate allocator and arm a new setup.
//!
//! CLAUDE.md compliance:
//!   - No hardcoded constants: every knob via `resolve_system_*`.
//!   - No scattered if/else: single dispatch helpers + early returns.
//!   - Asset-class agnostic: venue class derived from a dispatch table.

use std::collections::HashMap;
use std::time::Duration;

use qtss_confluence::{
    ConfluenceDirection, ConfluenceInputs, ConfluenceReading, ConfluenceWeights, DetectionVote,
};
use qtss_indicators::{atr, ema};
use qtss_setup_engine::{
    check_allocation, classify_alt_type, confluence_gate_should_open_with_reading,
    should_reverse_close, AllocatorContext, AllocatorLimits, CloseReason, Direction, GateConfig,
    GateContext, OpenSetupSummary, PositionGuard, PositionGuardConfig, Profile, RejectReason,
    SetupState, StructuralTarget, VenueClass,
};
use qtss_storage::v2_confluence::fetch_latest_v2_confluence;
use qtss_storage::{
    insert_v2_setup, insert_v2_setup_event, insert_v2_setup_rejection, list_enabled_engine_symbols,
    list_groups_for_symbol, list_open_v2_setups, list_recent_bars, resolve_system_f64,
    resolve_system_u64, resolve_worker_enabled_flag, update_v2_setup_state,
    DetectionOutcomeRepository, DetectionRow, EngineSymbolRow, V2DetectionRepository,
    V2SetupEventInsert, V2SetupInsert, V2SetupRejectionInsert, V2SetupRow,
};
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ---------- config ----------

#[derive(Debug, Clone)]
struct LoopConfig {
    enabled: bool,
    tick_interval_s: u64,
    arm_guven_threshold: f64,
    profiles: HashMap<Profile, ProfileConfig>,
    allocator: AllocatorLimits,
    venue_enabled: HashMap<VenueClass, bool>,
    // TP override (pragmatic behavior — all D/T/Q models share this)
    tp_override_enabled: bool,
    tp_override_guven_threshold: f64,
    tp_override_max_extension_r: f64,
    gate: GateLimits,
}

#[derive(Debug, Clone)]
struct GateLimits {
    enabled: bool,
    min_score: f64,
    min_direction_votes: u8,
    reject_on_regimes: Vec<String>,
    kill_switch_on: bool,
    news_blackout_on: bool,
}

#[derive(Debug, Clone)]
struct ProfileConfig {
    guard: PositionGuardConfig,
}

const PROFILES: [Profile; 3] = [Profile::T, Profile::Q, Profile::D];

async fn load_profile(pool: &PgPool, p: Profile) -> ProfileConfig {
    let slug = p.as_str();
    let guard = PositionGuardConfig {
        entry_sl_atr_mult: resolve_system_f64(
            pool,
            "setup",
            &format!("profile.{slug}.entry_sl_atr_mult"),
            "",
            1.0,
        )
        .await,
        ratchet_interval_secs: resolve_system_u64(
            pool,
            "setup",
            &format!("profile.{slug}.ratchet_interval_secs"),
            "",
            60,
            1,
            86_400,
        )
        .await as i64,
        target_ref_r: resolve_system_f64(
            pool,
            "setup",
            &format!("profile.{slug}.target_ref_r"),
            "",
            2.0,
        )
        .await,
        risk_pct: resolve_system_f64(
            pool,
            "setup",
            &format!("profile.{slug}.risk_pct"),
            "",
            0.5,
        )
        .await,
        max_concurrent: resolve_system_u64(
            pool,
            "setup",
            &format!("profile.{slug}.max_concurrent"),
            "",
            match p {
                Profile::T => 4,
                Profile::Q => 3,
                Profile::D => 2,
            },
            0,
            1000,
        )
        .await as u32,
        reverse_guven_threshold: resolve_system_f64(
            pool,
            "setup",
            &format!("profile.{slug}.reverse_guven_threshold"),
            "",
            match p {
                Profile::T => 0.65,
                Profile::Q => 0.55,
                Profile::D => 0.70,
            },
        )
        .await,
    };
    ProfileConfig { guard }
}

async fn load_config(pool: &PgPool) -> LoopConfig {
    let enabled =
        resolve_worker_enabled_flag(pool, "setup", "enabled", "QTSS_SETUP_ENABLED", false).await;
    let tick_interval_s =
        resolve_system_u64(pool, "setup", "tick_interval_s", "", 30, 5, 3600).await;

    let arm_guven_threshold =
        resolve_system_f64(pool, "setup", "arm.guven_threshold", "", 0.50).await;

    let mut profiles: HashMap<Profile, ProfileConfig> = HashMap::new();
    for p in PROFILES {
        profiles.insert(p, load_profile(pool, p).await);
    }

    let max_total_open_risk_pct =
        resolve_system_f64(pool, "setup", "risk.total_risk_pct", "", 6.0).await;
    let correlation_max_per_group =
        resolve_system_u64(pool, "setup", "risk.correlation.max_per_group", "", 2, 0, 100).await
            as u32;
    let correlation_same_direction_only =
        resolve_system_u64(pool, "setup", "risk.correlation.same_direction_only", "", 1, 0, 1)
            .await
            == 1;

    let mut max_concurrent_per_profile: HashMap<Profile, u32> = HashMap::new();
    for p in PROFILES {
        max_concurrent_per_profile.insert(p, profiles[&p].guard.max_concurrent);
    }

    let allocator = AllocatorLimits {
        max_total_open_risk_pct,
        max_concurrent_per_profile,
        correlation_max_per_group,
        correlation_same_direction_only,
    };

    let mut venue_enabled: HashMap<VenueClass, bool> = HashMap::new();
    venue_enabled.insert(
        VenueClass::Crypto,
        resolve_system_u64(pool, "setup", "venue.crypto.enabled", "", 1, 0, 1).await == 1,
    );
    venue_enabled.insert(
        VenueClass::Bist,
        resolve_system_u64(pool, "setup", "venue.bist.enabled", "", 0, 0, 1).await == 1,
    );

    // TP override (pragmatic behavior shared by all D/T/Q models)
    let tp_override_enabled =
        resolve_system_u64(pool, "setup", "tp_override.enabled", "", 1, 0, 1).await == 1;
    let tp_override_guven_threshold =
        resolve_system_f64(pool, "setup", "tp_override.guven_threshold", "", 0.60).await;
    let tp_override_max_extension_r =
        resolve_system_f64(pool, "setup", "tp_override.max_extension_r", "", 3.0).await;

    let gate = load_gate_limits(pool).await;

    LoopConfig {
        enabled,
        tick_interval_s,
        arm_guven_threshold,
        profiles,
        allocator,
        venue_enabled,
        tp_override_enabled,
        tp_override_guven_threshold,
        tp_override_max_extension_r,
        gate,
    }
}

async fn load_gate_limits(pool: &PgPool) -> GateLimits {
    let enabled =
        resolve_system_u64(pool, "setup", "confluence_gate.enabled", "", 1, 0, 1).await == 1;
    let min_score =
        resolve_system_f64(pool, "setup", "confluence_gate.min_score", "", 0.55).await;
    let min_direction_votes =
        resolve_system_u64(pool, "setup", "confluence_gate.min_direction_votes", "", 2, 0, 10)
            .await as u8;
    let kill_switch_on =
        resolve_system_u64(pool, "setup", "confluence_gate.kill_switch_on", "", 0, 0, 1).await
            == 1;
    let news_blackout_on =
        resolve_system_u64(pool, "setup", "confluence_gate.news_blackout_on", "", 0, 0, 1).await
            == 1;
    // reject_on_regimes stored as JSON array of strings.
    let reject_on_regimes: Vec<String> = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM system_config WHERE module='setup' AND config_key='confluence_gate.reject_on_regimes' LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
    .unwrap_or_default();
    GateLimits {
        enabled,
        min_score,
        min_direction_votes,
        reject_on_regimes,
        kill_switch_on,
        news_blackout_on,
    }
}

// ---------- dispatch helpers ----------

/// Map an `engine_symbols.interval` string to the profile that owns it.
/// Single source of truth — no scattered ifs elsewhere.
fn profile_from_timeframe(tf: &str) -> Option<Profile> {
    match tf {
        "15m" => Some(Profile::T),
        "1h" => Some(Profile::Q),
        "4h" => Some(Profile::D),
        _ => None,
    }
}

/// Map an exchange code to a venue class. Unknown venues → `None`,
/// caller skips the symbol.
fn venue_class_from_exchange(exchange: &str) -> Option<VenueClass> {
    let e = exchange.trim().to_ascii_lowercase();
    const CRYPTO_KEYS: &[&str] = &["binance", "bybit", "okx", "kucoin", "coinbase"];
    if CRYPTO_KEYS.iter().any(|k| e.contains(k)) {
        return Some(VenueClass::Crypto);
    }
    if e.contains("bist") {
        return Some(VenueClass::Bist);
    }
    None
}

fn direction_from_str(s: &str) -> Direction {
    match s {
        "long" => Direction::Long,
        "short" => Direction::Short,
        _ => Direction::Neutral,
    }
}

/// Infer a DetectionVote direction from `family` + `subkind`. Mirrors
/// the heuristic used elsewhere in the loop (subkind contains bull/bear).
fn vote_direction(det: &DetectionRow) -> ConfluenceDirection {
    let s = det.subkind.as_str();
    if s.contains("bull") || s.contains("long") || s.contains("bottom") || s.contains("accumulation") {
        ConfluenceDirection::Long
    } else if s.contains("bear") || s.contains("short") || s.contains("top") || s.contains("distribution") {
        ConfluenceDirection::Short
    } else {
        ConfluenceDirection::Neutral
    }
}

fn build_detection_votes(detections: &[DetectionRow]) -> Vec<DetectionVote> {
    detections
        .iter()
        .filter(|d| d.state == "confirmed" || d.state == "forming")
        .map(|d| DetectionVote {
            family: d.family.clone(),
            subkind: d.subkind.clone(),
            direction: vote_direction(d),
            structural_score: d.structural_score,
        })
        .collect()
}

/// Build a `ConfluenceReading` from the persisted `V2ConfluenceRow`.
/// This keeps the gate in sync with whatever score the rest of the
/// system already published — no double-scoring drift.
fn reading_from_row(
    guven: f32,
    erken_uyari: f32,
    direction: &str,
    layer_count: i32,
) -> ConfluenceReading {
    ConfluenceReading {
        erken_uyari: erken_uyari as f64,
        guven: guven as f64,
        direction: match direction {
            "long" => ConfluenceDirection::Long,
            "short" => ConfluenceDirection::Short,
            _ => ConfluenceDirection::Neutral,
        },
        layer_count: layer_count.max(0) as u32,
        details: vec![],
    }
}

// ---------- loop entry ----------

pub async fn v2_setup_loop(pool: PgPool) {
    info!("v2 setup loop spawned (gated on setup.enabled)");
    loop {
        let cfg = load_config(&pool).await;
        if !cfg.enabled {
            tokio::time::sleep(Duration::from_secs(cfg.tick_interval_s)).await;
            continue;
        }
        match run_pass(&pool, &cfg).await {
            Ok(()) => debug!("v2 setup pass complete"),
            Err(e) => warn!(%e, "v2 setup pass failed"),
        }
        tokio::time::sleep(Duration::from_secs(cfg.tick_interval_s)).await;
    }
}

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

async fn run_pass(pool: &PgPool, cfg: &LoopConfig) -> Result<(), BoxErr> {
    let symbols = list_enabled_engine_symbols(pool).await?;

    // Hydrate per-venue allocator contexts once per tick.
    let mut ctx_by_venue: HashMap<VenueClass, AllocatorContext> = HashMap::new();
    for (venue, enabled) in cfg.venue_enabled.iter() {
        if !enabled {
            continue;
        }
        let ctx = hydrate_context(pool, *venue).await?;
        ctx_by_venue.insert(*venue, ctx);
    }

    for sym in symbols {
        if !qtss_storage::is_backfill_ready(pool, sym.id).await {
            continue;
        }
        let Some(venue) = venue_class_from_exchange(&sym.exchange) else {
            continue;
        };
        if cfg.venue_enabled.get(&venue).copied() != Some(true) {
            continue;
        }
        let Some(profile) = profile_from_timeframe(&sym.interval) else {
            continue;
        };
        let Some(ctx) = ctx_by_venue.get_mut(&venue) else {
            continue;
        };
        if let Err(e) = process_symbol(pool, cfg, venue, profile, &sym, ctx).await {
            warn!(symbol = %sym.symbol, interval = %sym.interval, %e, "setup symbol failed");
        }
    }
    Ok(())
}

async fn hydrate_context(pool: &PgPool, venue: VenueClass) -> Result<AllocatorContext, BoxErr> {
    // Load every mode; the allocator filters by candidate.mode per call
    // so live and backtest caps stay independent (Faz 9C).
    let rows = list_open_v2_setups(pool, Some(venue.as_str()), None).await?;
    let mut open_setups: Vec<OpenSetupSummary> = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(profile) = Profile::from_str(&r.profile) else {
            continue;
        };
        let groups = list_groups_for_symbol(pool, &r.venue_class, &r.symbol)
            .await
            .unwrap_or_default();
        open_setups.push(OpenSetupSummary {
            profile,
            direction: direction_from_str(&r.direction),
            risk_pct: r.risk_pct.unwrap_or(0.0) as f64,
            correlation_groups: groups,
            mode: r.mode.clone(),
        });
    }
    Ok(AllocatorContext { open_setups })
}

// ---------- per-symbol ----------

async fn process_symbol(
    pool: &PgPool,
    cfg: &LoopConfig,
    venue: VenueClass,
    profile: Profile,
    sym: &EngineSymbolRow,
    ctx: &mut AllocatorContext,
) -> Result<(), BoxErr> {
    // 1. Load recent bars (enough for EMA200).
    let window = 260i64;
    let raw_bars = list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        window,
    )
    .await?;
    if raw_bars.len() < 30 {
        return Ok(());
    }
    // Chronological order (oldest first).
    let mut chronological = raw_bars;
    chronological.reverse();

    let closes: Vec<f64> = chronological
        .iter()
        .map(|b| b.close.to_f64().unwrap_or(0.0))
        .collect();
    let highs: Vec<f64> = chronological
        .iter()
        .map(|b| b.high.to_f64().unwrap_or(0.0))
        .collect();
    let lows: Vec<f64> = chronological
        .iter()
        .map(|b| b.low.to_f64().unwrap_or(0.0))
        .collect();

    let current_price = *closes.last().ok_or("empty closes")?;
    let ema50_series = ema(&closes, 50);
    let ema200_series = ema(&closes, 200);
    let atr_series = atr(&highs, &lows, &closes, 14);
    let ema50 = last_finite(&ema50_series);
    let ema200 = last_finite(&ema200_series);
    let atr_val = last_finite(&atr_series);

    // 2. Update any existing open setup for this (venue, exchange, symbol, timeframe, profile).
    update_open_setups(
        pool, cfg, venue, profile, sym, current_price, ctx,
    )
    .await?;

    // 3. Try to arm a new setup if none already open for this key + direction.
    let atr_usable = atr_val.map(|v| v > 0.0).unwrap_or(false);
    let emas_usable = ema50.is_some() && ema200.is_some();
    if !atr_usable || !emas_usable {
        return Ok(());
    }
    try_arm_new_setup(
        pool,
        cfg,
        venue,
        profile,
        sym,
        current_price,
        ema50.unwrap(),
        ema200.unwrap(),
        atr_val.unwrap(),
        ctx,
    )
    .await?;

    Ok(())
}

fn last_finite(xs: &[f64]) -> Option<f64> {
    xs.iter().rev().find(|v| v.is_finite()).copied()
}

// ---------- update path ----------

async fn update_open_setups(
    pool: &PgPool,
    cfg: &LoopConfig,
    venue: VenueClass,
    profile: Profile,
    sym: &EngineSymbolRow,
    current_price: f64,
    ctx: &mut AllocatorContext,
) -> Result<(), BoxErr> {
    // Re-query open setups from DB for this narrow key; cheaper and
    // avoids stale-in-memory drift inside a single pass.
    let open = list_open_v2_setups(pool, Some(venue.as_str()), None).await?;
    let matching: Vec<V2SetupRow> = open
        .into_iter()
        .filter(|r| {
            r.exchange == sym.exchange
                && r.symbol == sym.symbol
                && r.timeframe == sym.interval
                && r.profile == profile.as_str()
        })
        .collect();

    let pcfg = &cfg.profiles[&profile].guard;
    for row in matching {
        let dir = direction_from_str(&row.direction);
        let (entry, entry_sl, koruma, target_ref) = match (
            row.entry_price,
            row.entry_sl,
            row.koruma,
            row.target_ref,
        ) {
            (Some(e), Some(es), Some(k), Some(t)) => {
                (e as f64, es as f64, k as f64, t as f64)
            }
            _ => continue,
        };
        let mut guard = PositionGuard {
            entry,
            entry_sl,
            koruma,
            target_ref,
            target_ref2: None,
            direction: dir,
            structural: false,
        };

        // --- close checks (ordered; first hit wins) ---
        if let Some((reason, exit_price)) =
            evaluate_close(pool, cfg, profile, &guard, current_price, sym).await?
        {
            close_setup(pool, &row, reason, exit_price, ctx).await?;
            continue;
        }

        // --- ratchet ---
        let changed = guard.try_ratchet(current_price);
        let _ = pcfg; // reserved for future interval gating
        if changed {
            update_v2_setup_state(
                pool,
                row.id,
                SetupState::Active.as_str(),
                Some(guard.koruma as f32),
                None,
                None,
            )
            .await?;
            insert_v2_setup_event(
                pool,
                &V2SetupEventInsert {
                    setup_id: row.id,
                    event_type: "updated".to_string(),
                    payload: json!({
                        "koruma": guard.koruma,
                        "active_sl": guard.active_sl(),
                        "unrealized_r": guard.unrealized_r(current_price),
                        "price": current_price,
                    }),
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn evaluate_close(
    pool: &PgPool,
    cfg: &LoopConfig,
    profile: Profile,
    guard: &PositionGuard,
    price: f64,
    sym: &EngineSymbolRow,
) -> Result<Option<(CloseReason, f64)>, BoxErr> {
    let active_sl = guard.active_sl();
    // Stop-hit (long: price<=sl; short: price>=sl).
    let stop_hit = match guard.direction {
        Direction::Long => price <= active_sl,
        Direction::Short => price >= active_sl,
        Direction::Neutral => false,
    };
    if stop_hit {
        return Ok(Some((CloseReason::StopHit, active_sl)));
    }
    // Target-hit — with TP override: if structure still strong, extend.
    let target_hit = match guard.direction {
        Direction::Long => price >= guard.target_ref,
        Direction::Short => price <= guard.target_ref,
        Direction::Neutral => false,
    };
    if target_hit {
        // Check TP override: keep position open if guven is strong enough.
        if cfg.tp_override_enabled {
            let latest_conf =
                fetch_latest_v2_confluence(pool, &sym.exchange, &sym.symbol, &sym.interval).await?;
            let guven = latest_conf.as_ref().map(|c| c.guven as f64).unwrap_or(0.0);
            let extension_r = guard.unrealized_r(price);
            if guven >= cfg.tp_override_guven_threshold
                && extension_r < cfg.tp_override_max_extension_r
            {
                // Structure strong + within max extension → skip target close, let ratchet manage.
                debug!(
                    symbol = %sym.symbol,
                    guven = guven,
                    extension_r = extension_r,
                    "TP override: yapı güçlü, target aşılıyor"
                );
            } else {
                return Ok(Some((CloseReason::TargetHit, guard.target_ref)));
            }
        } else {
            return Ok(Some((CloseReason::TargetHit, guard.target_ref)));
        }
    }
    // Reverse-signal — needs latest confluence.
    let latest =
        fetch_latest_v2_confluence(pool, &sym.exchange, &sym.symbol, &sym.interval).await?;
    let Some(row) = latest else {
        return Ok(None);
    };
    let reading = ConfluenceReading {
        erken_uyari: row.erken_uyari as f64,
        guven: row.guven as f64,
        direction: direction_from_str(&row.direction),
        layer_count: row.layer_count as u32,
        details: vec![],
    };
    let threshold = cfg.profiles[&profile].guard.reverse_guven_threshold;
    if should_reverse_close(guard.direction, profile, &reading, threshold) {
        let exit = match guard.direction {
            Direction::Long => active_sl.max(price),
            Direction::Short => active_sl.min(price),
            Direction::Neutral => price,
        };
        return Ok(Some((CloseReason::ReverseSignal, exit)));
    }
    Ok(None)
}

/// Round-trip (entry + exit) taker commission expressed as a % of entry.
///
/// Single source so the open-time gate and the close-time P&L adjustment
/// can't drift apart. Routed through `resolve_commission_bps` so the
/// venue-specific override (`setup.commission.{venue}.taker_bps`) wins
/// over the global fallback.
async fn net_round_trip_commission_pct(pool: &PgPool, venue_class: &str) -> f64 {
    let taker_bps = qtss_storage::resolve_commission_bps(
        pool,
        venue_class,
        qtss_storage::CommissionSide::Taker,
        5.0,
    )
    .await;
    (taker_bps * 2.0) / 10_000.0 * 100.0
}

async fn close_setup(
    pool: &PgPool,
    row: &V2SetupRow,
    reason: CloseReason,
    exit_price: f64,
    ctx: &mut AllocatorContext,
) -> Result<(), BoxErr> {
    // Granular close state + P&L computation.
    let close_state = SetupState::from_close_reason(reason);
    // Net-of-commission pnl_pct — we subtract the round-trip taker fee so the
    // outcome labeler + Faz 9.3 trainer see the realized P&L the account
    // actually booked. Gross pnl at this point silently biased the win label
    // for marginal trades whose fees ate the edge (CLAUDE.md #2: all tunables
    // are config-driven).
    let round_trip_pct = net_round_trip_commission_pct(pool, &row.venue_class).await;
    let pnl_pct = row.entry_price.map(|ep| {
        let ep = ep as f64;
        if ep.abs() < 1e-12 { return 0.0f32; }
        let gross = match row.direction.as_str() {
            "long" => (exit_price - ep) / ep * 100.0,
            "short" => (ep - exit_price) / ep * 100.0,
            _ => 0.0,
        };
        (gross - round_trip_pct) as f32
    });

    update_v2_setup_state(
        pool,
        row.id,
        close_state.as_str(),
        None,
        Some(reason.as_str()),
        Some(exit_price as f32),
    )
    .await?;

    // Write pnl_pct to the setup row.
    if let Some(pnl) = pnl_pct {
        sqlx::query("UPDATE qtss_setups SET pnl_pct = $1 WHERE id = $2")
            .bind(pnl)
            .bind(row.id)
            .execute(pool)
            .await?;
    }

    insert_v2_setup_event(
        pool,
        &V2SetupEventInsert {
            setup_id: row.id,
            event_type: "closed".to_string(),
            payload: json!({
                "reason": reason.as_str(),
                "close_state": close_state.as_str(),
                "exit_price": exit_price,
                "pnl_pct": pnl_pct,
            }),
        },
    )
    .await?;
    // Record detection outcome for validator self-learning.
    record_detection_outcome(pool, row, &reason, exit_price).await;

    // Drop from local allocator context — first match removed.
    let dir = direction_from_str(&row.direction);
    if let Some(p) = Profile::from_str(&row.profile) {
        if let Some(pos) = ctx.open_setups.iter().position(|s| {
            s.profile == p && s.direction == dir && (s.risk_pct - row.risk_pct.unwrap_or(0.0) as f64).abs() < 1e-9
        }) {
            ctx.open_setups.remove(pos);
        }
    }
    Ok(())
}

/// Map close reason to outcome, compute P&L %, and persist.
async fn record_detection_outcome(
    pool: &PgPool,
    row: &V2SetupRow,
    reason: &CloseReason,
    exit_price: f64,
) {
    // Need a detection_id — try the FK column, fall back to raw_meta.
    let detection_id = row.detection_id.or_else(|| {
        row.raw_meta
            .get("detection_id")
            .and_then(|v| v.as_str())
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
    });
    let Some(det_id) = detection_id else {
        return; // No originating detection — nothing to record.
    };

    let outcome = match reason {
        CloseReason::TargetHit => "win",
        CloseReason::StopHit => "loss",
        CloseReason::ReverseSignal | CloseReason::Manual => "scratch",
    };

    // Same net-of-commission adjustment as close_setup — keeps the detection
    // outcome table and v2 setup row in sync so downstream stats don't drift.
    let round_trip_pct = net_round_trip_commission_pct(pool, &row.venue_class).await;
    let pnl_pct = row.entry_price.map(|ep| {
        let ep = ep as f64;
        if ep.abs() < 1e-12 {
            return 0.0_f32;
        }
        let gross = match row.direction.as_str() {
            "long" => (exit_price - ep) / ep * 100.0,
            "short" => (ep - exit_price) / ep * 100.0,
            _ => 0.0,
        };
        (gross - round_trip_pct) as f32
    });

    let duration_secs = row.created_at.signed_duration_since(chrono::DateTime::UNIX_EPOCH).num_seconds();
    let now_secs = chrono::Utc::now().signed_duration_since(chrono::DateTime::UNIX_EPOCH).num_seconds();
    let dur = Some(now_secs - duration_secs);

    let repo = DetectionOutcomeRepository::new(pool.clone());
    if let Err(e) = repo
        .record(
            det_id,
            Some(row.id),
            outcome,
            Some(reason.as_str()),
            pnl_pct,
            row.entry_price,
            Some(exit_price as f32),
            dur,
        )
        .await
    {
        warn!(%e, "record_detection_outcome failed");
    }
}

// ---------- arm path ----------

#[allow(clippy::too_many_arguments)]
async fn try_arm_new_setup(
    pool: &PgPool,
    cfg: &LoopConfig,
    venue: VenueClass,
    profile: Profile,
    sym: &EngineSymbolRow,
    price: f64,
    ema50: f64,
    ema200: f64,
    atr_val: f64,
    ctx: &mut AllocatorContext,
) -> Result<(), BoxErr> {
    // Load latest confluence.
    let latest =
        fetch_latest_v2_confluence(pool, &sym.exchange, &sym.symbol, &sym.interval).await?;
    let Some(conf) = latest else {
        return Ok(());
    };
    if (conf.guven as f64) < cfg.arm_guven_threshold {
        return Ok(());
    }
    let direction = direction_from_str(&conf.direction);
    if matches!(direction, Direction::Neutral) {
        return Ok(());
    }

    // Skip if we already have an open setup for (symbol, timeframe, profile, direction).
    // Duplicate check is cross-mode on purpose: migration 0171 already
    // includes `mode` in the partial unique index, so the duplicate guard
    // here exists only to short-circuit before the INSERT. Counting every
    // mode is harmless; a dup in any mode still means we shouldn't arm.
    let already_open = list_open_v2_setups(pool, Some(venue.as_str()), None).await?;
    let duplicate = already_open.iter().any(|r| {
        r.exchange == sym.exchange
            && r.symbol == sym.symbol
            && r.timeframe == sym.interval
            && r.profile == profile.as_str()
            && direction_from_str(&r.direction) == direction
    });
    if duplicate {
        return Ok(());
    }

    // P14 — opposite-direction guard. A single (exchange, symbol,
    // timeframe, profile) must never have both LONG and SHORT armed
    // or open at the same time. Operator caught the failure mode on
    // 2026-04-14: TBM `detect_setups` returns Bottom *and* Top sets
    // whenever each score clears the threshold independently, so
    // without this gate we armed two mutually-destructive setups on
    // the same candle at the same entry price (BTC 15m both sides
    // armed at 74242.50). The broader Wyckoff stack treats direction
    // as an *outcome of the active structure* — you cannot both
    // accumulate and distribute the same bar. Hard-skip here; DB-side
    // enforcement lives in migration 0078.
    let opposite = match direction {
        Direction::Long => Direction::Short,
        Direction::Short => Direction::Long,
        Direction::Neutral => Direction::Neutral,
    };
    if !matches!(opposite, Direction::Neutral) {
        let conflict = already_open.iter().any(|r| {
            r.exchange == sym.exchange
                && r.symbol == sym.symbol
                && r.timeframe == sym.interval
                && r.profile == profile.as_str()
                && direction_from_str(&r.direction) == opposite
        });
        if conflict {
            tracing::info!(
                symbol = %sym.symbol,
                interval = %sym.interval,
                profile = %profile.as_str(),
                requested = ?direction,
                blocked_by = ?opposite,
                "setup arm rejected: opposite-direction already open"
            );
            return Ok(());
        }
    }

    let pcfg = cfg.profiles[&profile].guard;

    // Try structural guard: use detection invalidation + target-engine
    // measured-move targets instead of ATR-based fallback.
    let det_repo = V2DetectionRepository::new(pool.clone());
    // Faz 9.8.25 — widen from 10 to 100 so structural vote tally has
    // enough signal. Elliott commonly emits bull + bear patterns in the
    // same window; with only 10 rows the vote often splits 5/5 and the
    // consensus layer rejects every setup.
    let detections = det_repo
        .list_for_chart(&sym.exchange, &sym.symbol, &sym.interval, 100)
        .await
        .unwrap_or_default();
    // Captured for raw_meta — the chart overlay uses these to label
    // TP lines with the formation-specific name ("MM 1.0x" / "Pat
    // 1.618x" / "ABCD 1.272x") instead of generic TP1/TP2. Kept in
    // sync with the guard so what the user sees in the GUI matches
    // what the engine actually armed with.
    let mut structural_targets_meta: Vec<serde_json::Value> = Vec::new();
    let mut structural_subkind: Option<String> = None;

    let guard = {
        // Find the best confirmed detection matching our direction.
        let best_det = detections.iter().find(|d| {
            let det_dir = match d.family.as_str() {
                _ if d.subkind.contains("bull") => Direction::Long,
                _ if d.subkind.contains("bear") => Direction::Short,
                _ => Direction::Neutral,
            };
            (d.state == "confirmed" || d.state == "forming")
                && det_dir == direction
                && d.structural_score >= 0.60
        });

        match best_det {
            Some(det) => {
                let inv_price = det.invalidation_price.to_f64().unwrap_or(0.0);
                // Extract targets from anchors using measured-move formula.
                let targets = compute_structural_targets(det, direction);
                if targets.is_empty() {
                    PositionGuard::new(price, atr_val, &pcfg, direction)
                } else {
                    info!(
                        symbol = %sym.symbol,
                        subkind = %det.subkind,
                        inv = inv_price,
                        tp1 = targets[0].price,
                        "structural guard from detection"
                    );
                    structural_subkind = Some(det.subkind.clone());
                    structural_targets_meta = targets
                        .iter()
                        .map(|t| {
                            json!({
                                "price": t.price,
                                "weight": t.weight,
                                "label": t.label,
                            })
                        })
                        .collect();
                    PositionGuard::new_structural(
                        price, inv_price, &targets, atr_val, &pcfg, direction,
                    )
                }
            }
            None => PositionGuard::new(price, atr_val, &pcfg, direction),
        }
    };

    let groups = list_groups_for_symbol(pool, venue.as_str(), &sym.symbol)
        .await
        .unwrap_or_default();
    // Mode tag for allocator — mirrors the final `setup_mode` picked
    // just before INSERT (Faz 9B: inherit from triggering detection).
    // Pre-computed here so the allocator's mode-scoped caps evaluate
    // the candidate against its OWN mode pool, not a mixed pool.
    let candidate_mode: String = detections
        .iter()
        .find(|d| {
            let det_dir = if d.subkind.contains("bull") {
                Direction::Long
            } else if d.subkind.contains("bear") {
                Direction::Short
            } else {
                Direction::Neutral
            };
            det_dir == direction && (d.state == "confirmed" || d.state == "forming")
        })
        .map(|d| d.mode.clone())
        .unwrap_or_else(|| "live".to_string());
    let candidate = OpenSetupSummary {
        profile,
        direction,
        risk_pct: pcfg.risk_pct,
        correlation_groups: groups.clone(),
        mode: candidate_mode.clone(),
    };

    // Faz 9.1 — Classic confluence gate (veto + consensus + score).
    // Pre-computed reading from the persisted `qtss_v2_confluence` row
    // keeps the gate consistent with the published score.
    if cfg.gate.enabled {
        // Faz 9.8.25 — pre-filter structural votes to the direction the
        // confluence row already decided on. `score_confluence` already
        // did the weighted tally when it wrote the `qtss_v2_confluence`
        // row, so the gate's Layer-2 consensus should only verify that
        // *at least one* structural pattern supports that direction, not
        // re-tally a split bull/bear window.
        let reading_dir = match conf.direction.as_str() {
            "long" => ConfluenceDirection::Long,
            "short" => ConfluenceDirection::Short,
            _ => ConfluenceDirection::Neutral,
        };
        let all_votes = build_detection_votes(&detections);
        let votes: Vec<DetectionVote> = if reading_dir == ConfluenceDirection::Neutral {
            all_votes
        } else {
            all_votes
                .into_iter()
                .filter(|v| v.direction == reading_dir)
                .collect()
        };
        let inputs = ConfluenceInputs {
            tbm_score: None,
            tbm_confidence: None,
            detections: votes,
            onchain: None,
        };
        let gctx = GateContext {
            inputs,
            regime_label: None,
            kill_switch_on: cfg.gate.kill_switch_on,
            stale_data: false,
            news_blackout: cfg.gate.news_blackout_on,
        };
        let gcfg = GateConfig {
            weights: ConfluenceWeights::default(),
            min_score: cfg.gate.min_score,
            min_direction_votes: cfg.gate.min_direction_votes,
            reject_on_regimes: cfg.gate.reject_on_regimes.clone(),
        };
        let reading = reading_from_row(
            conf.guven,
            conf.erken_uyari,
            &conf.direction,
            conf.layer_count,
        );
        if let Err(rej) = confluence_gate_should_open_with_reading(&gctx, &gcfg, reading) {
            let reason = RejectReason::from_veto_kind(rej.kind);
            debug!(
                symbol = %sym.symbol,
                kind = rej.kind.as_str(),
                detail = %rej.reason,
                "confluence gate rejected"
            );
            record_rejection(pool, cfg, venue, profile, sym, direction, conf.id, reason).await?;
            return Ok(());
        }
    }

    if let Err(reason) = check_allocation(&cfg.allocator, ctx, &candidate) {
        record_rejection(pool, cfg, venue, profile, sym, direction, conf.id, reason).await?;
        return Ok(());
    }

    // Commission gate: reject if expected profit < round-trip commission cost.
    // Faz 8 step 1 — route through the shared `resolve_commission_bps`
    // so Wyckoff + D/T/Q agree on a single venue-aware source of truth
    // (MEMORY gap list). Order: `commission.{venue_class}.taker_bps` →
    // `commission.taker_bps` → 5 bps fallback.
    {
        let round_trip_pct = net_round_trip_commission_pct(pool, venue.as_str()).await;
        // Safety multiplier — `profit_pct = 1.01 × round_trip` is effectively
        // break-even once slippage + funding are counted. Default 1.5 forces a
        // real edge; operators can tighten or loosen from Config GUI.
        let min_mult = qtss_storage::resolve_system_f64(
            pool,
            "setup",
            "commission_gate.min_profit_multiple",
            "",
            1.5,
        )
        .await;
        let gate_pct = round_trip_pct * min_mult;
        let entry = guard.entry;
        let target = guard.target_ref;
        let profit_pct = match direction {
            Direction::Long => ((target - entry) / entry) * 100.0,
            Direction::Short => ((entry - target) / entry) * 100.0,
            Direction::Neutral => 0.0,
        };
        if profit_pct <= gate_pct {
            record_rejection(pool, cfg, venue, profile, sym, direction, conf.id, RejectReason::CommissionGate).await?;
            return Ok(());
        }
    }

    let alt_type = classify_alt_type(direction, ema50, ema200, price);

    // Faz 9.4.3 — evaluate circuit breaker before inference. If open,
    // skip the AI gate entirely (equivalent to gate_enabled=false).
    let breaker = crate::ai_inference::evaluate_circuit_breaker(pool).await;
    let breaker_open = breaker.state == "open";

    // Faz 9.3.3 + 9.3.5 — ask the inference sidecar for P(win) and
    // persist a full telemetry row to `qtss_ml_predictions`. `None` on
    // any soft failure keeps the setup path alive (shadow mode). The
    // gate only vetoes when `ai.inference.gate_enabled=true` AND the
    // sidecar returned an actual score below `min_score`.
    let ai_cfg = crate::ai_inference::InferenceConfig::load(pool).await;

    // Faz 9.4.4 — traffic ramp: gate_pct controls what fraction of
    // setups actually go through the gate vs shadow.
    let effective_gate = ai_cfg.gate_enabled && !breaker_open && {
        let pct = resolve_system_f64(pool, "ai", "inference.gate_pct", "", 0.0).await;
        pct >= 1.0 || rand::random::<f64>() < pct
    };

    // Shadow kill-switch: skip sidecar entirely if gate is off AND
    // operator disabled shadow logging (CLAUDE.md #2).
    let should_call_sidecar =
        ai_cfg.enabled && (effective_gate || ai_cfg.log_shadow_predictions);

    let ai_window_minutes = 15;
    let features_by_source = if should_call_sidecar {
        qtss_storage::fetch_latest_features_by_source(
            pool,
            &sym.exchange,
            &sym.symbol,
            &sym.interval,
            ai_window_minutes,
        )
        .await
        .unwrap_or(None)
    } else {
        None
    };

    let inference_start = std::time::Instant::now();
    // Faz 9C — fan-out to active + every role='shadow' model in one
    // sidecar round-trip. The active entry drives the gate; shadow
    // entries are written as audit-only prediction rows below.
    let multi_response = match features_by_source.as_ref() {
        Some(f) => crate::ai_inference::score_multi(&ai_cfg, f).await,
        None => None,
    };
    let inference_latency_ms = inference_start.elapsed().as_millis() as i32;
    // Adapter: the rest of this function was built against the flat
    // ScoreResponse shape. We reuse it by projecting the active entry
    // back into that struct so the gate logic / SHAP path stays intact.
    let ai_response = multi_response.as_ref().and_then(|m| {
        m.active.as_ref().map(|a| crate::ai_inference::ScoreResponse {
            score: a.score,
            model_family: a.model_family.clone(),
            model_version: a.model_version.clone(),
            model_name: a.model_family.clone(),
            feature_spec_version: a.feature_spec_version.clone(),
            missing_features: a.missing_features,
            n_features: a.n_features,
        })
    });

    // Compute feature hash for reproducibility (Faz 9.3.5).
    let feat_hash = features_by_source
        .as_ref()
        .and_then(crate::ai_inference::feature_hash);

    // Faz 9.3.4 — SHAP top-10 (optional, diagnostic).
    let shap_json: Option<serde_json::Value> = match (
        &ai_response,
        features_by_source.as_ref(),
        ai_cfg.explain_enabled,
    ) {
        (Some(_), Some(f), true) => crate::ai_inference::explain(&ai_cfg, f)
            .await
            .and_then(|r| serde_json::to_value(&r.shap_top10).ok()),
        _ => None,
    };

    // Determine decision for the prediction row.
    let ai_decision = ai_response.as_ref().map(|r| {
        if !effective_gate {
            "shadow"
        } else if r.score >= ai_cfg.min_score {
            "pass"
        } else {
            "block"
        }
    });

    // Persist prediction row (Faz 9.3.5). Inserted with setup_id=None;
    // if a setup is later born we back-fill the FK via `attach_setup_id`.
    let prediction_id: Option<Uuid> = match ai_response.as_ref() {
        Some(r) => {
            let insert = qtss_storage::MlPredictionInsert {
                setup_id: None,
                detection_id: None, // linked post-insert if needed
                exchange: sym.exchange.clone(),
                symbol: sym.symbol.clone(),
                timeframe: sym.interval.clone(),
                model_name: r.model_name.clone(),
                model_version: r.model_version.clone(),
                model_sha: None,
                feature_spec_version: r.feature_spec_version.clone(),
                feature_hash: feat_hash.clone(),
                score: r.score as f32,
                threshold: ai_cfg.min_score as f32,
                gate_enabled: effective_gate,
                decision: ai_decision.unwrap_or("shadow").to_string(),
                shap_top10: shap_json.clone(),
                latency_ms: inference_latency_ms,
                sidecar_url: ai_cfg.sidecar_url.clone(),
                role: "active".to_string(),
            };
            match qtss_storage::insert_ml_prediction(pool, &insert).await {
                Ok(id) => Some(id),
                Err(e) => {
                    warn!(%e, "insert_ml_prediction failed (non-fatal)");
                    None
                }
            }
        }
        None => None,
    };

    // Faz 9C — persist shadow predictions (audit-only, never gate). One
    // row per shadow model the sidecar scored. Soft-fail per row so one
    // bad shadow doesn't sink the others or the setup path.
    if let Some(multi) = multi_response.as_ref() {
        for sh in &multi.shadows {
            let insert = qtss_storage::MlPredictionInsert {
                setup_id: None,
                detection_id: None,
                exchange: sym.exchange.clone(),
                symbol: sym.symbol.clone(),
                timeframe: sym.interval.clone(),
                model_name: sh.model_family.clone(),
                model_version: sh.model_version.clone(),
                model_sha: None,
                feature_spec_version: sh.feature_spec_version.clone(),
                feature_hash: feat_hash.clone(),
                score: sh.score as f32,
                threshold: ai_cfg.min_score as f32,
                gate_enabled: false,
                decision: "shadow".to_string(),
                shap_top10: None,
                latency_ms: inference_latency_ms,
                sidecar_url: ai_cfg.sidecar_url.clone(),
                role: "shadow".to_string(),
            };
            if let Err(e) = qtss_storage::insert_ml_prediction(pool, &insert).await {
                warn!(%e, version = %sh.model_version, "shadow prediction insert failed");
            }
        }
    }

    let ai_score = ai_response.as_ref().map(|r| r.score as f32);
    if effective_gate {
        if let Some(r) = ai_response.as_ref() {
            if r.score < ai_cfg.min_score {
                debug!(
                    symbol = %sym.symbol,
                    score = r.score,
                    min = ai_cfg.min_score,
                    "ai_gate rejected setup"
                );
                record_rejection(
                    pool, cfg, venue, profile, sym, direction, conf.id,
                    RejectReason::AiGate,
                )
                .await?;
                return Ok(());
            }
        }
    }

    // Faz 9.5 — LLM tiebreaker for the uncertain zone.
    let llm_cfg = crate::llm_judge::LlmConfig::load(pool).await;
    let llm_verdict = if llm_cfg.enabled {
        if let Some(r) = ai_response.as_ref() {
            if llm_cfg.score_in_uncertain_zone(r.score) {
                // Build context from available detections + SHAP.
                let best_det = detections.iter().find(|d| {
                    (d.state == "confirmed" || d.state == "forming")
                        && d.structural_score >= 0.50
                });
                let shap_top5: Vec<crate::llm_judge::ShapEntry> = shap_json
                    .as_ref()
                    .and_then(|v| serde_json::from_value::<Vec<crate::llm_judge::ShapEntry>>(v.clone()).ok())
                    .unwrap_or_default()
                    .into_iter()
                    .take(5)
                    .collect();
                let ctx = crate::llm_judge::SetupContext {
                    symbol: sym.symbol.clone(),
                    timeframe: sym.interval.clone(),
                    family: best_det.map(|d| d.family.clone()).unwrap_or_default(),
                    subkind: best_det.map(|d| d.subkind.clone()).unwrap_or_default(),
                    ai_score: r.score,
                    shap_top5,
                    regime: best_det
                        .and_then(|d| d.regime.as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| "unknown".into()),
                    structural_score: best_det.map(|d| d.structural_score as f64).unwrap_or(0.0),
                    confidence: best_det.and_then(|d| d.confidence.map(|c| c as f64)).unwrap_or(0.0),
                };
                crate::llm_judge::judge(&llm_cfg, &ctx).await
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Persist LLM verdict if obtained.
    if let (Some(verdict), Some(pred_id)) = (&llm_verdict, prediction_id) {
        let _ = qtss_storage::insert_llm_verdict(
            pool,
            &qtss_storage::LlmVerdictInsert {
                prediction_id: pred_id,
                provider: llm_cfg.provider.clone(),
                model: llm_cfg.model.clone(),
                prompt_version: llm_cfg.prompt_version.clone(),
                verdict: verdict.verdict.clone(),
                confidence: Some(verdict.confidence as f32),
                reasoning: Some(verdict.reasoning.clone()),
                input_tokens: verdict.input_tokens,
                output_tokens: verdict.output_tokens,
                latency_ms: verdict.latency_ms as i32,
                raw_response: verdict.raw_response.clone(),
            },
        )
        .await;
    }

    // Apply LLM verdict if it has a blocking opinion.
    if let Some(ref v) = llm_verdict {
        if v.verdict == "block" {
            debug!(
                symbol = %sym.symbol,
                confidence = v.confidence,
                reasoning = %v.reasoning,
                "llm_tiebreaker rejected setup"
            );
            record_rejection(
                pool, cfg, venue, profile, sym, direction, conf.id,
                RejectReason::LlmBlock,
            )
            .await?;
            return Ok(());
        }
        // "pass" → proceed, "abstain" → fall through to classic gate
    }

    // Faz 9.8.AI-1 — pick the primary detection whose direction matches the
    // confluence reading and has the highest structural_score. This detection_id
    // is what `qtss_features_snapshot.detection_id` is keyed by, so persisting
    // it unblocks the `v_qtss_training_set` join and the LightGBM trainer.
    let primary_detection = detections
        .iter()
        .filter(|d| {
            let det_dir = if d.subkind.contains("bull") {
                Direction::Long
            } else if d.subkind.contains("bear") {
                Direction::Short
            } else {
                Direction::Neutral
            };
            det_dir == direction && (d.state == "confirmed" || d.state == "forming")
        })
        .max_by(|a, b| {
            a.structural_score
                .partial_cmp(&b.structural_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    let primary_detection_id: Option<Uuid> = primary_detection.map(|d| d.id);
    // Faz 9B backfill fix — setup inherits mode from the triggering
    // detection. historical_progressive_scan tags detections 'backtest';
    // live detection orchestrator uses worker.runtime_mode. Without this
    // propagation every setup defaulted to the DB column default ('dry')
    // and the backfill orchestrator's mode='backtest' plateau probe
    // never saw growth (31M detections → 0 backtest setups).
    let setup_mode: String = primary_detection
        .map(|d| d.mode.clone())
        .unwrap_or_else(|| "live".to_string());

    let row = V2SetupInsert {
        venue_class: venue.as_str().to_string(),
        exchange: sym.exchange.clone(),
        symbol: sym.symbol.clone(),
        timeframe: sym.interval.clone(),
        profile: profile.as_str().to_string(),
        alt_type: alt_type.map(|a| a.as_str().to_string()),
        state: SetupState::Active.as_str().to_string(),
        direction: match direction {
            Direction::Long => "long",
            Direction::Short => "short",
            Direction::Neutral => "neutral",
        }
        .to_string(),
        confluence_id: Some(conf.id),
        entry_price: Some(guard.entry as f32),
        entry_sl: Some(guard.entry_sl as f32),
        koruma: Some(guard.koruma as f32),
        target_ref: Some(guard.target_ref as f32),
        risk_pct: Some(pcfg.risk_pct as f32),
        raw_meta: json!({
            "profile": profile.as_str(),
            "alt_type": alt_type.map(|a| a.as_str()),
            "ema50": ema50,
            "ema200": ema200,
            "atr": atr_val,
            "guven": conf.guven,
            "correlation_groups": groups,
            "ai": ai_response.as_ref().map(|r| json!({
                "model_family": r.model_family,
                "model_version": r.model_version,
                "score": r.score,
                "missing_features": r.missing_features,
                "n_features": r.n_features,
            })),
            "structural_targets": structural_targets_meta,
            "structural_subkind": structural_subkind,
        }),
        ai_score,
        detection_id: primary_detection_id,
        mode: setup_mode,
    };
    let id: Uuid = match insert_v2_setup(pool, &row).await {
        Ok(id) => id,
        Err(qtss_storage::error::StorageError::DuplicateSetup) => {
            debug!(symbol = %sym.symbol, "duplicate open setup — skipped");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    // Faz 9.3.5 — back-fill the setup FK on the prediction row now
    // that the setup is born.
    if let Some(pred_id) = prediction_id {
        if let Err(e) = qtss_storage::attach_ml_prediction_setup_id(pool, pred_id, id).await {
            warn!(%e, "attach_ml_prediction_setup_id failed (non-fatal)");
        }
    }

    // Faz 9.7.8 — enqueue a public-broadcast outbox row. Failure here
    // is non-fatal: the setup itself is already persisted, and the
    // publisher loop tolerates a missing outbox entry (it only ships
    // what's actually enqueued). Keeping it non-fatal preserves the
    // setup engine's forward progress if notify gets wedged.
    if let Err(e) = qtss_storage::enqueue_setup_broadcast(pool, id).await {
        warn!(%e, setup_id = %id, "enqueue_setup_broadcast failed (non-fatal)");
    }

    insert_v2_setup_event(
        pool,
        &V2SetupEventInsert {
            setup_id: id,
            event_type: "opened".to_string(),
            payload: json!({
                "profile": profile.as_str(),
                "alt_type": alt_type.map(|a| a.as_str()),
                "entry": guard.entry,
                "entry_sl": guard.entry_sl,
                "koruma": guard.koruma,
                "target_ref": guard.target_ref,
                "target_ref2": guard.target_ref2,
                "structural": guard.structural,
                "risk_pct": pcfg.risk_pct,
                "confluence_id": conf.id,
                "ema50": ema50,
                "ema200": ema200,
                "atr": atr_val,
                "direction": direction.as_str(),
            }),
        },
    )
    .await?;

    ctx.open_setups.push(candidate);
    info!(
        exchange = %sym.exchange,
        symbol = %sym.symbol,
        timeframe = %sym.interval,
        profile = %profile.as_str(),
        direction = %direction.as_str(),
        "v2 setup armed"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn record_rejection(
    pool: &PgPool,
    _cfg: &LoopConfig,
    venue: VenueClass,
    profile: Profile,
    sym: &EngineSymbolRow,
    direction: Direction,
    confluence_id: Uuid,
    reason: RejectReason,
) -> Result<(), BoxErr> {
    insert_v2_setup_rejection(
        pool,
        &V2SetupRejectionInsert {
            venue_class: venue.as_str().to_string(),
            exchange: sym.exchange.clone(),
            symbol: sym.symbol.clone(),
            timeframe: sym.interval.clone(),
            profile: profile.as_str().to_string(),
            direction: direction.as_str().to_string(),
            reject_reason: reason.as_str().to_string(),
            confluence_id: Some(confluence_id),
            raw_meta: json!({ "source": "v2_setup_loop" }),
        },
    )
    .await?;
    Ok(())
}

// ── Structural target extraction from detection anchors ─────────
//
// Convention (CRITICAL):
//   * `sign = direction.sign()` → +1.0 for Long, -1.0 for Short.
//   * Target formula is always `anchor_price + sign * offset`.
//     For Long → target > anchor (up), for Short → target < anchor (down).
//   * `new_structural()` re-validates target side vs entry and falls
//     back to ATR if wrong — but silent fallback hides formula bugs, so
//     every handler below is unit-testable-side-correct by construction.
//
// Handler dispatch (first match wins):
//   classical:  double_top/bottom → MM; head_and_shoulders → neckline
//                MM; wedge/flag/channel/rectangle/diamond/broadening/
//                triangle/cup_handle/rounding → pattern-height projection.
//   harmonic:   XABCD family → AD-retracement targets.
//   elliott:    impulse → wave-1 projection; zigzag → 1.272×C-leg trend
//                resumption; flat → 0.618×C-leg; triangle → width thrust;
//                leading_diagonal → 1×range continuation; ending_diagonal
//                → full retrace to wedge origin.
//   candle:     pattern-height projection (1×, 2×).
//   gap:        gap-size projection (1×, 2.618×).
//   wyckoff:    spring/upthrust → range projection.

/// Classical pattern families that share the "pattern height → breakout
/// projection" target model. Listed most-specific first.
const CLASSICAL_HEIGHT_PREFIXES: &[&str] = &[
    "rising_wedge", "falling_wedge",
    "bull_flag", "bear_flag", "pennant",
    "ascending_channel", "descending_channel",
    "ascending_triangle", "descending_triangle", "symmetrical_triangle",
    "rectangle",
    "diamond_top", "diamond_bottom",
    "broadening",
    "cup_and_handle", "inverse_cup_and_handle",
    "rounding_top", "rounding_bottom",
    // Added after audit — these families are emitted by qtss-classical
    // but previously fell through to the ATR-based fallback. Anchors
    // are compatible with the generic pattern-height projection.
    "scallop_bullish", "scallop_bearish",
    "measured_move_abcd",
];

/// Extract measured-move targets from a detection's anchors.
/// Covers every formation family with at least two fallback tiers; no
/// scattered match arms (CLAUDE.md #1). Returns empty vec only when the
/// anchors are structurally unusable — the caller then falls back to
/// ATR-based PositionGuard.
pub(crate) fn compute_structural_targets(det: &DetectionRow, direction: Direction) -> Vec<StructuralTarget> {
    let anchors: Vec<serde_json::Value> = match serde_json::from_value(det.anchors.clone()) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    compute_structural_targets_raw(
        &anchors,
        det.subkind.as_str(),
        det.family.as_str(),
        direction,
        det.invalidation_price.to_f64().unwrap_or(0.0),
        &det.raw_meta,
    )
}

/// Value-based sibling of `compute_structural_targets`. Used by the
/// classical feature extractor (Faz 9.3) so LightGBM sees normalised
/// target geometry (count, R-multiples) as input features, not just
/// the shape ordinal. Single pure function shared with the setup
/// arming path so the two can't drift.
pub(crate) fn compute_structural_targets_raw(
    anchors: &[serde_json::Value],
    subkind: &str,
    family: &str,
    direction: Direction,
    inv_price: f64,
    raw_meta: &serde_json::Value,
) -> Vec<StructuralTarget> {
    if anchors.is_empty() {
        return Vec::new();
    }

    // Helper: extract price from anchor JSON {"price": "72000.50", ...}
    let price_of = |a: &serde_json::Value| -> Option<f64> {
        a.get("price")
            .and_then(|v| v.as_str().or_else(|| v.as_f64().map(|_| "")).and_then(|s| {
                if s.is_empty() { v.as_f64() } else { s.parse::<f64>().ok() }
            }))
    };

    let sign = direction.sign();
    if sign == 0.0 {
        return Vec::new();
    }

    // ---------------- classical: double top / bottom ----------------
    // 3 anchors [H1/L1, T/N, H2/L2]. Measured move projects *from the
    // neckline* (T = anchors[1]) by pattern height, in the trade's
    // direction. Fixed sign convention — previous revision used
    // `neck - sign*h` which produced wrong-side targets for both
    // variants and silently ATR-fell-back.
    if anchors.len() >= 3
        && (subkind.starts_with("double_top") || subkind.starts_with("double_bottom"))
    {
        if let (Some(extreme), Some(neck)) = (price_of(&anchors[0]), price_of(&anchors[1])) {
            let height = (extreme - neck).abs();
            if height > 0.0 {
                return vec![
                    StructuralTarget { price: neck + sign * height,         weight: 0.80, label: "MM 1.0x" },
                    StructuralTarget { price: neck + sign * height * 1.618, weight: 0.50, label: "MM 1.618x" },
                ];
            }
        }
    }

    // ---------------- classical: head & shoulders -------------------
    // 5 anchors [S1, N1, H, N2, S2]. Same sign fix as above.
    if anchors.len() >= 5 && subkind.contains("head_and_shoulders") {
        if let (Some(head), Some(n1), Some(n2)) =
            (price_of(&anchors[2]), price_of(&anchors[1]), price_of(&anchors[3]))
        {
            let neckline = (n1 + n2) / 2.0;
            let height = (head - neckline).abs();
            if height > 0.0 {
                return vec![
                    StructuralTarget { price: neckline + sign * height,         weight: 0.80, label: "MM 1.0x" },
                    StructuralTarget { price: neckline + sign * height * 1.618, weight: 0.50, label: "MM 1.618x" },
                ];
            }
        }
    }

    // ---------------- classical: triple top / bottom ----------------
    // 5 anchors [P1, V1, P2, V2, P3]. Neckline = avg(V1, V2); height =
    // distance from avg peak (P1..P3 avg) to neckline. Measured move
    // projects from neckline ± height in the trade's direction. Weight
    // tier slightly above double_top/bottom because triple confirmation
    // carries stronger structural basis.
    if anchors.len() >= 5
        && (subkind.starts_with("triple_top") || subkind.starts_with("triple_bottom"))
    {
        let p1 = price_of(&anchors[0]);
        let v1 = price_of(&anchors[1]);
        let p2 = price_of(&anchors[2]);
        let v2 = price_of(&anchors[3]);
        let p3 = price_of(&anchors[4]);
        if let (Some(p1), Some(v1), Some(p2), Some(v2), Some(p3)) = (p1, v1, p2, v2, p3) {
            let neck = (v1 + v2) / 2.0;
            let peak = (p1 + p2 + p3) / 3.0;
            let height = (peak - neck).abs();
            if height > 0.0 {
                return vec![
                    StructuralTarget { price: neck + sign * height,         weight: 0.85, label: "MM 1.0x" },
                    StructuralTarget { price: neck + sign * height * 1.618, weight: 0.55, label: "MM 1.618x" },
                ];
            }
        }
    }

    // ---------------- classical: measured move ABCD -----------------
    // 4 anchors [A, B, C, D]. Classic AB=CD projection: target from D
    // equal to the AB leg length in the trade's direction. Extension
    // ladder uses 1.272× and 1.618× of AB (common Fib continuation
    // multiples for this pattern).
    if anchors.len() >= 4 && subkind.starts_with("measured_move_abcd") {
        if let (Some(a), Some(b), Some(d)) =
            (price_of(&anchors[0]), price_of(&anchors[1]), price_of(&anchors[3]))
        {
            let ab = (b - a).abs();
            if ab > 0.0 {
                return vec![
                    StructuralTarget { price: d + sign * ab * 1.000, weight: 0.80, label: "ABCD 1.0x" },
                    StructuralTarget { price: d + sign * ab * 1.272, weight: 0.55, label: "ABCD 1.272x" },
                    StructuralTarget { price: d + sign * ab * 1.618, weight: 0.40, label: "ABCD 1.618x" },
                ];
            }
        }
    }

    // ---------------- classical: v-spike reversals ------------------
    // v_top/v_bottom: 3 anchors. SL at extreme, target = recent pivot
    // reclaim distance projected in direction.
    if anchors.len() >= 3 && (subkind.starts_with("v_top") || subkind.starts_with("v_bottom")) {
        if let (Some(tip), Some(neck)) = (price_of(&anchors[1]), price_of(&anchors[0])) {
            let height = (tip - neck).abs();
            if height > 0.0 {
                return vec![
                    StructuralTarget { price: neck + sign * height * 0.618, weight: 0.65, label: "V 0.618x" },
                    StructuralTarget { price: neck + sign * height * 1.0,   weight: 0.45, label: "V 1.0x" },
                ];
            }
        }
    }

    // ---------------- classical: generic pattern-height ------------
    // Wedges, flags, channels, rectangles, diamonds, broadening, cup &
    // handle, rounding, scallops. Project pattern height in breakout
    // direction from the *entry* (last anchor = breakout pivot), NOT
    // the invalidation edge. Earlier version used `inv + sign*h`, but
    // for these families `height = max - min ≈ entry - inv`, so
    // `inv + h ≈ entry` — TP1 collapsed onto the Entry label and TP2
    // sat only ~0.6× height above entry. Projecting from entry yields
    // the correct "target distance = pattern height" measured-move.
    if family == "classical"
        && CLASSICAL_HEIGHT_PREFIXES.iter().any(|p| subkind.starts_with(p))
    {
        let prices: Vec<f64> = anchors.iter().filter_map(|a| price_of(a)).collect();
        if prices.len() >= 2 {
            let max = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let min = prices.iter().cloned().fold(f64::INFINITY, f64::min);
            let height = max - min;
            let entry = *prices.last().unwrap();
            if height > 0.0 && entry > 0.0 {
                return vec![
                    StructuralTarget { price: entry + sign * height,         weight: 0.70, label: "Pat 1.0x" },
                    StructuralTarget { price: entry + sign * height * 1.618, weight: 0.45, label: "Pat 1.618x" },
                ];
            }
        }
    }

    // ---------------- harmonic: XABCD family -----------------------
    // Generic handler covers gartley, butterfly, bat, crab, cypher,
    // shark, five_0, ab=cd, three_drives (min 5 pivots; AD = first→last).
    if family == "harmonic" && anchors.len() >= 5 {
        let a_price = price_of(&anchors[1]);
        let d_price = price_of(anchors.last().unwrap());
        if let (Some(a), Some(d)) = (a_price, d_price) {
            let ad = (a - d).abs();
            if ad > 0.0 {
                return vec![
                    StructuralTarget { price: d + sign * ad * 0.382, weight: 0.70, label: "AD 0.382" },
                    StructuralTarget { price: d + sign * ad * 0.618, weight: 0.85, label: "AD 0.618" },
                    StructuralTarget { price: d + sign * ad * 1.000, weight: 0.55, label: "AD 1.000" },
                ];
            }
        }
    }

    // ---------------- elliott: impulse 1-2-3-4-5 -------------------
    if subkind.contains("impulse") && anchors.len() >= 6 {
        if let (Some(p0), Some(p1), Some(p4)) =
            (price_of(&anchors[0]), price_of(&anchors[1]), price_of(&anchors[4]))
        {
            let w1 = (p1 - p0).abs();
            if w1 > 0.0 {
                return vec![
                    StructuralTarget { price: p4 + sign * w1 * 1.000, weight: 0.70, label: "Fib 1.0x" },
                    StructuralTarget { price: p4 + sign * w1 * 1.618, weight: 0.85, label: "Fib 1.618x" },
                ];
            }
        }
    }

    // ---------------- elliott: zigzag/flat corrections --------------
    // 4 anchors [0, A, B, C]. After C, trend RESUMES in the broader
    // direction — confluence `direction` already reflects that. Zigzag
    // implies sharper pullback so targets are larger than flat.
    if anchors.len() >= 4
        && (subkind.starts_with("zigzag") || subkind.starts_with("flat_"))
    {
        if let (Some(c_start), Some(c_end)) = (price_of(&anchors[2]), price_of(&anchors[3])) {
            let c_leg = (c_end - c_start).abs();
            if c_leg > 0.0 {
                let mult = if subkind.starts_with("zigzag") { 1.272 } else { 0.618 };
                return vec![
                    StructuralTarget { price: c_end + sign * c_leg * mult,           weight: 0.60, label: "C-leg mm" },
                    StructuralTarget { price: c_end + sign * c_leg * (mult + 0.618), weight: 0.40, label: "C-leg ext" },
                ];
            }
        }
    }

    // ---------------- elliott: triangle thrust ---------------------
    // 6 anchors. Thrust magnitude = widest point of triangle, projected
    // from E in the pattern's breakout direction.
    if subkind.starts_with("triangle_") && anchors.len() >= 6 {
        let prices: Vec<f64> = anchors.iter().filter_map(|a| price_of(a)).collect();
        if prices.len() >= 6 {
            let max = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let min = prices.iter().cloned().fold(f64::INFINITY, f64::min);
            let width = max - min;
            let e = prices[5];
            if width > 0.0 {
                return vec![
                    StructuralTarget { price: e + sign * width * 0.618, weight: 0.55, label: "Thrust 0.618x" },
                    StructuralTarget { price: e + sign * width * 1.000, weight: 0.75, label: "Thrust 1.0x" },
                ];
            }
        }
    }

    // ---------------- elliott: diagonals ---------------------------
    if subkind.starts_with("leading_diagonal") && anchors.len() >= 6 {
        if let (Some(p0), Some(p5)) = (price_of(&anchors[0]), price_of(&anchors[5])) {
            let range = (p5 - p0).abs();
            if range > 0.0 {
                return vec![
                    StructuralTarget { price: p5 + sign * range * 0.618, weight: 0.60, label: "Cont 0.618x" },
                    StructuralTarget { price: p5 + sign * range * 1.000, weight: 0.75, label: "Cont 1.0x" },
                ];
            }
        }
    }
    if subkind.starts_with("ending_diagonal") && anchors.len() >= 6 {
        if let (Some(p0), Some(p5)) = (price_of(&anchors[0]), price_of(&anchors[5])) {
            let range = (p5 - p0).abs();
            if range > 0.0 {
                // Sharp reversal: full retrace back to wedge start, then
                // 1.618× extension beyond. Direction flips vs formation.
                return vec![
                    StructuralTarget { price: p0,                       weight: 0.75, label: "Retrace p0" },
                    StructuralTarget { price: p0 - sign * range * 0.618, weight: 0.45, label: "Retrace ext" },
                ];
            }
        }
    }

    // ---------------- candle: single/multi-bar patterns ------------
    // Anchors: [open_first, close_last]. Pattern height = |close - inv|
    // where inv = pattern extreme. Candle targets carry lower weight
    // (less structural basis than swing patterns).
    if family == "candle" && anchors.len() >= 2 {
        if let Some(close) = price_of(anchors.last().unwrap()) {
            let height = (close - inv_price).abs();
            if height > 0.0 {
                return vec![
                    StructuralTarget { price: close + sign * height * 1.0, weight: 0.50, label: "Candle 1.0x" },
                    StructuralTarget { price: close + sign * height * 2.0, weight: 0.30, label: "Candle 2.0x" },
                ];
            }
        }
    }

    // ---------------- gap family -----------------------------------
    // Anchors: [pre_gap_close, post_gap_open] (or equivalent). Gap size
    // drives continuation projection. Exhaustion gaps are still
    // handled here — direction comes from confluence which already
    // accounts for exhaustion reversal semantics.
    if family == "gap" && anchors.len() >= 2 {
        if let (Some(a), Some(b)) = (price_of(&anchors[0]), price_of(&anchors[1])) {
            let gap = (b - a).abs();
            if gap > 0.0 {
                return vec![
                    StructuralTarget { price: b + sign * gap * 1.000, weight: 0.65, label: "Gap 1.0x" },
                    StructuralTarget { price: b + sign * gap * 2.618, weight: 0.40, label: "Gap 2.618x" },
                ];
            }
        }
    }

    // ---------------- wyckoff: spring / upthrust --------------------
    if subkind.contains("spring") || subkind.contains("upthrust") {
        if let (Some(top), Some(bot)) = (
            raw_meta.get("range_top").and_then(|v| v.as_f64()),
            raw_meta.get("range_bottom").and_then(|v| v.as_f64()),
        ) {
            let range_h = (top - bot).abs();
            if range_h > 0.0 && inv_price > 0.0 {
                return vec![
                    StructuralTarget { price: inv_price + sign * range_h * 0.5, weight: 0.70, label: "Range 0.5x" },
                    StructuralTarget { price: inv_price + sign * range_h * 1.0, weight: 0.85, label: "Range 1.0x" },
                ];
            }
        }
    }

    Vec::new()
}
