// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `iq_d_candidate_loop` — FAZ 25 PR-25C.
//!
//! Reads active `iq_structures` (PR-25B's tracker output) and turns
//! each one into a `qtss_setups` row with profile='iq_d'. User's
//! locked entry priority (docs/FAZ_25_IQ_PREDICTIVE.md §4):
//!
//!   * Tier 1 — W1: nascent_impulse hit, earliest tradable signal,
//!                  highest R:R, hardest to confirm
//!   * Tier 2 — W2: forming_impulse hit, ride W3 from W2 retrace end
//!   * Tier 3 — W3: full motive emerging mid-impulse, last-resort
//!
//! Each tier writes a different subkind so the chart and the
//! allocator can treat them differently:
//!   profile=iq_d, raw_meta.entry_tier ∈ {W1, W2, W3}
//!
//! Entry / SL / TP follow Elliott structural rules:
//!   * Bull motive  : entry near W2 retrace, SL at W0 (impulse start),
//!                    TP at projected W5 high (W3.length × 1.618 from
//!                    W2 if available, else W3 high × 1.272)
//!   * Bear mirror.
//!
//! The IQ-T candidate loop already wired raw_meta.iq_structure_id
//! and tries to resolve parent_setup_id by joining on it; once this
//! loop runs, that join lights up and PR-25E's allocator can see
//! the parent → child setup chain.
//!
//! Strict-isolation principle: existing T/D setups untouched.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{info, warn};

const DEFAULT_TICK_SECS: u64 = 90;

pub async fn iq_d_candidate_loop(pool: PgPool) {
    info!("iq_d_candidate_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            continue;
        }
        match run_tick(&pool).await {
            Ok((scanned, created)) => info!(
                scanned, created,
                "iq_d_candidate tick ok"
            ),
            Err(e) => warn!(%e, "iq_d_candidate tick failed"),
        }
        let secs = load_tick_secs(&pool).await;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module='iq_d_candidate' AND config_key='enabled'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return true; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

async fn load_min_dip_score(pool: &PgPool) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='major_dip' AND config_key='min_score_for_setup'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.55; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("value")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.55)
        .clamp(0.0, 1.0)
}

async fn load_tick_secs(pool: &PgPool) -> u64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module='iq_d_candidate' AND config_key='tick_secs'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return DEFAULT_TICK_SECS; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("secs").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_TICK_SECS).max(30)
}

#[derive(Debug, Clone)]
struct ParentStructure {
    id: uuid::Uuid,
    exchange: String,
    segment: String,
    symbol: String,
    timeframe: String,
    slot: i16,
    direction: i16,
    state: String,
    current_wave: String,
    current_stage: String,
    structure_anchors: Value,
    started_at: DateTime<Utc>,
}

async fn run_tick(pool: &PgPool) -> anyhow::Result<(usize, usize)> {
    // Read every TRACKING / CANDIDATE iq_structures row. Each one is
    // a candidate parent — the entry tier depends on its current_wave.
    let parents = sqlx::query(
        r#"SELECT id, exchange, segment, symbol, timeframe, slot, direction,
                  state, current_wave, current_stage, structure_anchors,
                  started_at
             FROM iq_structures
            WHERE state IN ('candidate','tracking')
            ORDER BY last_advanced_at DESC
            LIMIT 200"#,
    )
    .fetch_all(pool)
    .await?;

    let mut scanned = 0usize;
    let mut created = 0usize;
    for r in parents {
        let p = ParentStructure {
            id: r.try_get("id")?,
            exchange: r.try_get("exchange").unwrap_or_default(),
            segment: r.try_get("segment").unwrap_or_default(),
            symbol: r.try_get("symbol").unwrap_or_default(),
            timeframe: r.try_get("timeframe").unwrap_or_default(),
            slot: r.try_get("slot").unwrap_or(0),
            direction: r.try_get("direction").unwrap_or(0),
            state: r.try_get("state").unwrap_or_default(),
            current_wave: r.try_get("current_wave").unwrap_or_default(),
            current_stage: r.try_get("current_stage").unwrap_or_default(),
            structure_anchors: r.try_get("structure_anchors").unwrap_or(Value::Null),
            started_at: r.try_get("started_at").unwrap_or_else(|_| Utc::now()),
        };
        scanned += 1;

        // Pick entry tier from current wave. W1/W2/W3 are tradable
        // entries; later waves (W4/W5/A/B/C) are riding the move and
        // IQ-T handles those entries on the child TF.
        let tier = match p.current_wave.as_str() {
            "W1" => "W1",
            "W2" => "W2",
            "W3" => "W3",
            _ => continue,
        };

        // Compute entry / SL / TP from the parent's anchors.
        let Some((entry, sl, tp)) = compute_entry_targets(&p, tier) else {
            continue;
        };
        if entry <= 0.0 || sl <= 0.0 || tp <= 0.0 {
            continue;
        }
        if !targets_are_sane(p.direction, entry, sl, tp) {
            continue;
        }

        // FAZ 25.4.A — Wyckoff↔Elliott alignment gate. If the major-
        // dip composite includes a wyckoff_alignment component AND
        // the latest Wyckoff event for this (sym, tf) actively
        // CONTRADICTS the Elliott wave intent, refuse to spawn the
        // setup. Examples: Spring + W2 = good; UTAD + W3 = bad
        // (volume signals top, structure says bull continuation).
        // Default behaviour: only block hard CONFLICT; missing data
        // or neutral events fall through (no penalty).
        let require_wyckoff = sqlx::query_scalar::<_, Option<bool>>(
            r#"SELECT (value->>'enabled')::boolean FROM system_config
                WHERE module='iq_d_candidate' AND config_key='require_wyckoff_alignment'"#,
        )
        .fetch_optional(pool).await.ok().flatten().flatten().unwrap_or(true);
        if require_wyckoff {
            let alignment_meta = sqlx::query(
                r#"SELECT components->'wyckoff_event' AS wy
                     FROM major_dip_candidates
                    WHERE exchange=$1 AND segment=$2
                      AND symbol=$3 AND timeframe=$4
                    ORDER BY candidate_time DESC LIMIT 1"#,
            )
            .bind(&p.exchange).bind(&p.segment).bind(&p.symbol).bind(&p.timeframe)
            .fetch_optional(pool).await.ok().flatten();
            let conflicts = alignment_meta
                .map(|r| {
                    let wy: Value = r.try_get("wy").unwrap_or(Value::Null);
                    let subkind = wy
                        .get("subkind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    // Bear-tagged event under a bull entry tier =
                    // structural conflict; refuse the setup.
                    let bear_event = subkind.ends_with("_bear");
                    let bull_setup = p.direction == 1;
                    bear_event && bull_setup
                })
                .unwrap_or(false);
            if conflicts {
                continue;
            }
        }

        // FAZ 25.4.G — macro 4-phase cycle alignment gate.
        // Optional veto when the active cycle phase contradicts the
        // long bias (Distribution / Markdown means market is topping
        // or already declining — opening a long is structurally
        // wrong). Default off so the gate is advisory; flip
        // `iq_d_candidate.require_cycle_alignment` to true to
        // enforce.
        let require_cycle = sqlx::query_scalar::<_, Option<bool>>(
            r#"SELECT (value->>'value')::boolean FROM system_config
                WHERE module='iq_d_candidate'
                  AND config_key='require_cycle_alignment'"#,
        )
        .fetch_optional(pool).await.ok().flatten().flatten().unwrap_or(false);
        if require_cycle {
            let cycle_meta = sqlx::query(
                r#"SELECT components->'cycle_context' AS ctx
                     FROM major_dip_candidates
                    WHERE exchange=$1 AND segment=$2
                      AND symbol=$3 AND timeframe=$4
                    ORDER BY candidate_time DESC LIMIT 1"#,
            )
            .bind(&p.exchange).bind(&p.segment).bind(&p.symbol).bind(&p.timeframe)
            .fetch_optional(pool).await.ok().flatten();
            let cycle_blocks = cycle_meta
                .map(|r| {
                    let ctx: Value = r.try_get("ctx").unwrap_or(Value::Null);
                    let phase = ctx
                        .get("phase")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    // Distribution / Markdown vetoes IQ-D long.
                    matches!(phase, "distribution" | "markdown")
                })
                .unwrap_or(false);
            if cycle_blocks {
                continue;
            }
        }

        // FAZ 25.3.B — Major Dip composite gate. The score is computed
        // by major_dip_candidate_loop and tells us whether the latest
        // structural low has enough multi-channel confirmation
        // (Wyckoff volume + Fib zone + multi-TF alignment + sentiment +
        // structural completion) to be worth opening a setup against.
        // User: "ezbere olmuş ... dalganın başladığını bize söyleyecek
        // ek bilgi veriler gerekir." Below the threshold = composite
        // doesn't agree the dip is real → skip rather than spawning
        // mechanically.
        let min_dip_score = load_min_dip_score(pool).await;
        if min_dip_score > 0.0 {
            let dip_score = sqlx::query_scalar::<_, Option<f64>>(
                r#"SELECT score FROM major_dip_candidates
                    WHERE exchange=$1 AND segment=$2
                      AND symbol=$3 AND timeframe=$4
                    ORDER BY candidate_time DESC LIMIT 1"#,
            )
            .bind(&p.exchange)
            .bind(&p.segment)
            .bind(&p.symbol)
            .bind(&p.timeframe)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
            .flatten()
            .unwrap_or(0.0);
            if dip_score < min_dip_score {
                continue;
            }
        }

        // Symbol lock check (PR-25E) — when the structure tracker
        // marked a previous structure invalidated, the symbol stays
        // locked until a brand-new candidate appears. Don't write a
        // fresh iq_d setup while the lock is active.
        let locked = sqlx::query(
            "SELECT 1 FROM iq_symbol_locks
              WHERE exchange = $1 AND segment = $2 AND symbol = $3
              LIMIT 1",
        )
        .bind(&p.exchange)
        .bind(&p.segment)
        .bind(&p.symbol)
        .fetch_optional(pool)
        .await?;
        if locked.is_some() && p.state != "candidate" {
            // 'candidate' state means a brand-new structure just
            // showed up — that itself clears the lock (see
            // iq_structure_tracker_loop::unlock_symbol). Anything
            // else hits the gate.
            continue;
        }

        // Cross-pipeline hedge gate (mirrors iq_t logic): refuse to
        // spawn a counter-direction iq_d setup on the same symbol+tf
        // when another setup is already armed in the opposite
        // direction.
        let direction_str_check = if p.direction == 1 { "long" } else { "short" };
        let opposite = if direction_str_check == "long" { "short" } else { "long" };
        let conflict = sqlx::query(
            r#"SELECT 1 FROM qtss_setups
                WHERE exchange = $1 AND symbol = $2 AND timeframe = $3
                  AND state IN ('armed','active')
                  AND direction = $4
                LIMIT 1"#,
        )
        .bind(&p.exchange)
        .bind(&p.symbol)
        .bind(&p.timeframe)
        .bind(opposite)
        .fetch_optional(pool)
        .await?;
        if conflict.is_some() {
            continue;
        }

        // Idempotency key — one iq_d setup per (structure_id, tier)
        // so we don't spawn a new row on every tick.
        let key = format!("iq_d:{}:{}", p.id, tier);
        let dup = sqlx::query("SELECT 1 FROM qtss_setups WHERE idempotency_key = $1 LIMIT 1")
            .bind(&key)
            .fetch_optional(pool)
            .await?;
        if dup.is_some() {
            continue;
        }

        let direction_str = if p.direction == 1 { "long" } else { "short" };
        let raw_meta = json!({
            "iq_structure_id": p.id.to_string(),
            "entry_tier": tier,
            "parent_slot": p.slot,
            "parent_state": p.state,
            "parent_current_wave": p.current_wave,
            "parent_current_stage": p.current_stage,
        });

        if let Err(e) = sqlx::query(
            r#"INSERT INTO qtss_setups
                  (venue_class, exchange, symbol, timeframe, profile, state,
                   direction, entry_price, entry_sl, target_ref,
                   raw_meta, mode, idempotency_key)
               VALUES ('crypto', $1, $2, $3, 'iq_d', 'armed',
                       $4, $5::real, $6::real, $7::real,
                       $8, 'dry', $9)"#,
        )
        .bind(&p.exchange)
        .bind(&p.symbol)
        .bind(&p.timeframe)
        .bind(direction_str)
        .bind(entry)
        .bind(sl)
        .bind(tp)
        .bind(&raw_meta)
        .bind(&key)
        .execute(pool)
        .await
        {
            warn!(symbol=%p.symbol, %e, "iq_d write failed");
            continue;
        }
        // Real-time push so SSE-connected GUI tabs invalidate cache
        // immediately instead of waiting on the 30s react-query poll.
        let _ = sqlx::query("SELECT pg_notify('qtss_iq_changed', $1)")
            .bind(json!({
                "kind": "iq_setup",
                "exchange": p.exchange,
                "segment": p.segment,
                "symbol": p.symbol,
                "timeframe": p.timeframe,
                "profile": "iq_d",
            }).to_string())
            .execute(pool)
            .await;
        created += 1;
    }
    Ok((scanned, created))
}

/// Compute (entry, sl, tp) for an IQ-D setup based on the entry tier.
///
/// The structure_anchors JSON is an array of {wave_label, price, ...}.
/// We index into it by wave_label so the parent's exact anchor list
/// (might be 4 anchors for nascent, 5 for forming, 6 for full motive)
/// doesn't matter — we just look up the labels we need.
fn compute_entry_targets(
    parent: &ParentStructure,
    tier: &str,
) -> Option<(f64, f64, f64)> {
    let arr = parent.structure_anchors.as_array()?;
    let price_at = |label: &str| -> Option<f64> {
        arr.iter()
            .find(|a| a.get("wave_label").and_then(|v| v.as_str()) == Some(label))
            .and_then(|a| a.get("price").and_then(|p| p.as_f64()))
    };
    let bull = parent.direction == 1;
    let w0 = price_at("W0")?;
    let w1 = price_at("W1")?;
    let w2 = price_at("W2");
    let w3 = price_at("W3");

    // W1 leg sign carries direction (positive for bull, negative for
    // bear). All entry / SL / TP arithmetic below trusts this sign so
    // the bear branch is just the bull branch with `leg < 0`.
    let leg = w1 - w0;

    // Buffer used by the W3 breakout tier — 0.3% above (bull) / below
    // (bear) the W1 high so a wick doesn't accidentally fill the
    // setup right at the round number.
    const W3_BREAKOUT_BUFFER_PCT: f64 = 0.003;

    match tier {
        // W1 tier — only the W0→W1 leg is in. Don't enter at the W1
        // peak (that was the prior bug: long ETH 1w opened at $4099
        // because entry = w1). Instead we set a LIMIT at the
        // anticipated W2 retrace zone (50% of W0→W1) — the same place
        // a discretionary trader would buy the dip. SL just past W0
        // (impulse origin invalid below). TP = W1 + 1.618×leg
        // (canonical W3 extension).
        "W1" => {
            let entry = w1 - 0.5 * leg;
            let sl = w0;
            let tp = w1 + 1.618 * leg;
            Some((entry, sl, tp))
        }
        // W2 tier — the retrace already printed; entry at W2 itself
        // is correct (we're catching the bottom of the pullback).
        // SL just past W0; TP = W2 + 1.618×leg, with the existing
        // sanity fallback if the projection lands on the wrong side
        // of entry (e.g. extreme retracements).
        "W2" => {
            let w2 = w2?;
            let entry = w2;
            let sl = w0;
            let tp = w2 + 1.618 * leg;
            let target = if (bull && tp > entry) || (!bull && tp < entry) {
                tp
            } else {
                w1 + leg
            };
            Some((entry, sl, target))
        }
        // W3 tier — W3 has printed. The OLD code armed `entry = w3`
        // (the W3 PEAK) which produced the ETH 1w long at $4099 vs
        // $2300 spot regression. The CORRECT structural entry is the
        // W1 BREAKOUT level: a long fires only when price clears the
        // prior W1 high (proves W3 is in motion). For late entries
        // where price has already pulled back below W1, a limit at
        // W1 + buffer behaves like "wait for the next test of the
        // breakout" — much better R:R than chasing the W3 peak.
        // SL at W2 (Elliott invalidation: W3 cannot start below W1's
        // high, and W2 must hold). TP = W3 anchor + leg, the equality
        // projection of W5 from the realised W3 close.
        "W3" => {
            let w3 = w3?;
            let w2 = w2?;
            let entry = if bull {
                w1 * (1.0 + W3_BREAKOUT_BUFFER_PCT)
            } else {
                w1 * (1.0 - W3_BREAKOUT_BUFFER_PCT)
            };
            let sl = w2;
            let tp = w3 + leg;
            Some((entry, sl, tp))
        }
        _ => None,
    }
}

/// Sanity check: SL must be on the LOSS side of entry, TP must be
/// on the WIN side. Otherwise the row would arm but with reversed
/// stops — useless and noisy.
fn targets_are_sane(direction: i16, entry: f64, sl: f64, tp: f64) -> bool {
    if direction == 1 {
        sl < entry && tp > entry
    } else {
        sl > entry && tp < entry
    }
}
