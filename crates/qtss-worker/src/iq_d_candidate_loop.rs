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

    match tier {
        // W1 entry — earliest. Take the price at the candidate W1
        // anchor (which is the latest pivot), SL just below the W0
        // start (impulse origin), TP at a Fib extension projection.
        "W1" => {
            let entry = w1;
            let sl = w0;
            // TP1 = W1 + 1.618 * (W1 - W0) (canonical W3 extension)
            let leg = w1 - w0;
            let tp = w1 + 1.618 * leg;
            // For a bear motive `leg` is negative so tp lands lower
            // — direction sign carries through naturally.
            Some((entry, sl, tp))
        }
        // W2 entry — wait for W2 retrace to print, then ride W3.
        // Entry is W2 price (retrace bottom for bull); SL is W0;
        // TP is the projected W3 extension.
        "W2" => {
            let w2 = w2?;
            let entry = w2;
            let sl = w0;
            let leg = w1 - w0;
            let tp = w2 + 1.618 * leg;
            // Quick sanity: if TP wound up on the wrong side of
            // entry (bull TP must be > entry), fall back to W1 + leg.
            let target = if (bull && tp > entry) || (!bull && tp < entry) {
                tp
            } else {
                w1 + leg
            };
            Some((entry, sl, target))
        }
        // W3 entry — already mid-impulse. Entry mid-W3, SL at W2
        // (the recent correction low/high), TP at projected W5.
        "W3" => {
            let w3 = w3?;
            let w2 = w2?;
            let entry = w3;
            let sl = w2;
            // W5 ≈ W3 + (W1 length) is a reasonable conservative
            // target — Elliott "equality" baseline.
            let leg = w1 - w0;
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
