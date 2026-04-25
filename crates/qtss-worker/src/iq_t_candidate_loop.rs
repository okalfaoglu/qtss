// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `iq_t_candidate_loop` — FAZ 25 PR-25D.
//!
//! Reads active `iq_structures` (the parent IQ-D context that
//! PR-25B's tracker maintains) and looks for micro-impulse signals
//! on the CHILD timeframe inside the parent's current correction
//! leg. When found, writes a `qtss_setups` row with profile='iq_t'
//! and parent_setup_id linked to any matching IQ-D setup.
//!
//! Key idea (user's design): the BC leg of a 1d ABC contains a full
//! 5-wave impulse on 4h. Don't trade the 1d leg directly — wait for
//! the 4h sub-W2 end (or sub-W1 confirmation) and ride to sub-W5.
//! Tighter SL, multiple opportunities per parent leg, IQ-T position
//! is 1/3 of IQ-D risk budget.
//!
//! Strict-isolation principle (FAZ 25 §0): existing T/D allocator
//! profiles untouched. iq_t setups are tagged separately and
//! invisible to the legacy setup pipeline.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{info, warn};

const DEFAULT_TICK_SECS: u64 = 60;

pub async fn iq_t_candidate_loop(pool: PgPool) {
    info!("iq_t_candidate_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            continue;
        }
        match run_tick(&pool).await {
            Ok((scanned, created)) => info!(
                scanned, created,
                "iq_t_candidate tick ok"
            ),
            Err(e) => warn!(%e, "iq_t_candidate tick failed"),
        }
        let secs = load_tick_secs(&pool).await;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

// ─── config ───────────────────────────────────────────────────────────

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module='iq_t_candidate' AND config_key='enabled'",
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
        "SELECT value FROM system_config WHERE module='iq_t_candidate' AND config_key='tick_secs'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return DEFAULT_TICK_SECS; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("secs").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_TICK_SECS).max(20)
}

async fn load_min_anchor_score(pool: &PgPool) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='iq_t_candidate' AND config_key='min_anchor_score'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.40; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("value").and_then(|v| v.as_f64()).unwrap_or(0.40)
}

async fn load_child_tf(pool: &PgPool, parent_tf: &str) -> Option<String> {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='iq_t_candidate' AND config_key='parent_to_child_tf'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return None; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("map")
        .and_then(|m| m.get(parent_tf))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

// ─── data shapes ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ParentStructure {
    id: uuid::Uuid,
    exchange: String,
    segment: String,
    symbol: String,
    timeframe: String,
    slot: i16,
    direction: i16,
    current_wave: String,
    current_stage: String,
    structure_anchors: Value,
}

#[derive(Debug, Clone)]
struct ChildMotive {
    direction: i8,
    anchors: Value,
    end_time: DateTime<Utc>,
    score: f64,
}

// ─── tick body ────────────────────────────────────────────────────────

async fn run_tick(pool: &PgPool) -> anyhow::Result<(usize, usize)> {
    let min_score = load_min_anchor_score(pool).await;

    // Pull every actively-tracked IQ structure whose current wave is
    // a CORRECTION (W2, W4, A, B, C). Only those allow IQ-T entries —
    // see the matrix in docs/FAZ_25_IQ_PREDICTIVE.md §5.
    let parents = sqlx::query(
        r#"SELECT id, exchange, segment, symbol, timeframe, slot, direction,
                  current_wave, current_stage, structure_anchors
             FROM iq_structures
            WHERE state = 'tracking'
              AND current_wave IN ('W2','W4','A','B','C')
            ORDER BY last_advanced_at DESC
            LIMIT 100"#,
    )
    .fetch_all(pool)
    .await?;

    let mut scanned = 0usize;
    let mut created = 0usize;
    for r in parents {
        let parent = ParentStructure {
            id: r.try_get("id")?,
            exchange: r.try_get("exchange").unwrap_or_default(),
            segment: r.try_get("segment").unwrap_or_default(),
            symbol: r.try_get("symbol").unwrap_or_default(),
            timeframe: r.try_get("timeframe").unwrap_or_default(),
            slot: r.try_get("slot").unwrap_or(0),
            direction: r.try_get("direction").unwrap_or(0),
            current_wave: r.try_get("current_wave").unwrap_or_default(),
            current_stage: r.try_get("current_stage").unwrap_or_default(),
            structure_anchors: r.try_get("structure_anchors").unwrap_or(Value::Null),
        };
        scanned += 1;

        // Resolve the child timeframe for this parent.
        let Some(child_tf) = load_child_tf(pool, &parent.timeframe).await else {
            continue;
        };

        // Determine the EXPECTED direction of the next wave (the one
        // IQ-T would ride). Bullish IQ-D currently in W2 (down) → next
        // is W3 (up); B (up rally inside ABC) → next is C (down). The
        // direction matrix mirrors docs/FAZ_25_IQ_PREDICTIVE.md §5.
        let Some(expected_dir) = expected_next_wave_direction(&parent) else {
            continue;
        };

        // Find the freshest CHILD-TF micro-motive that points the same
        // way as the next expected wave. We accept full motives (best
        // signal) or impulse_forming_{bull,bear} (W4 in, W5 forming).
        let child = match find_matching_child_motive(
            pool,
            &parent,
            &child_tf,
            expected_dir,
            min_score,
        )
        .await?
        {
            Some(c) => c,
            None => continue,
        };

        // Avoid duplicates: if an iq_t setup already exists for this
        // parent + child motive end_time, skip (idempotency).
        let already = sqlx::query(
            r#"SELECT 1 FROM qtss_setups
                WHERE profile = 'iq_t'
                  AND parent_setup_id IS NOT DISTINCT FROM
                      (SELECT id FROM qtss_setups
                        WHERE profile='iq_d'
                          AND raw_meta->>'iq_structure_id' = $1::text
                        LIMIT 1)
                  AND raw_meta->>'child_motive_end_time' = $2
                LIMIT 1"#,
        )
        .bind(parent.id.to_string())
        .bind(child.end_time.to_rfc3339())
        .fetch_optional(pool)
        .await?;
        if already.is_some() {
            continue;
        }

        if let Err(e) = write_iq_t_setup(pool, &parent, &child_tf, &child, expected_dir).await {
            warn!(symbol=%parent.symbol, %e, "iq_t write failed");
            continue;
        }
        created += 1;
    }

    Ok((scanned, created))
}

// ─── direction matrix ─────────────────────────────────────────────────

/// Expected direction of the NEXT wave that IQ-T would trade, given
/// the parent IQ structure's current wave + stage. Mirrors the
/// matrix in docs/FAZ_25_IQ_PREDICTIVE.md §5.
///
/// Returns `Some(+1)` for long-bias next leg, `Some(-1)` for short-
/// bias, `None` when the current wave is not a tradeable inflection.
fn expected_next_wave_direction(parent: &ParentStructure) -> Option<i16> {
    // Bullish parent direction = +1, bearish = -1.
    let bull = parent.direction == 1;
    match parent.current_wave.as_str() {
        // Inside an impulse: W2 / W4 corrections end → next leg is
        // the same direction as the parent.
        "W2" | "W4" => Some(if bull { 1 } else { -1 }),
        // Inside an ABC: A is opposite to parent (bull motive's A
        // is down), B is back toward parent (bull motive's B is up
        // rally), C is opposite again. After EACH leg ends, IQ-T
        // rides the NEXT.
        "A" => Some(if bull { 1 } else { -1 }),  // after A, B rallies counter-correction
        "B" => Some(if bull { -1 } else { 1 }),  // after B, C resumes correction
        "C" => Some(if bull { 1 } else { -1 }),  // after C, new W1 in parent direction
        _ => None,
    }
}

// ─── child motive search ──────────────────────────────────────────────

async fn find_matching_child_motive(
    pool: &PgPool,
    parent: &ParentStructure,
    child_tf: &str,
    expected_dir: i16,
    min_score: f64,
) -> anyhow::Result<Option<ChildMotive>> {
    // Look at the freshest child-TF detection in the time window
    // since the parent's current wave started. We accept:
    //   * full `motive` rows (Pine port confirmed 5-wave impulse)
    //   * `impulse_forming_{bull,bear}` (W5 forming — earlier signal)
    let last_anchor_time: Option<DateTime<Utc>> = parent
        .structure_anchors
        .as_array()
        .and_then(|arr| arr.last())
        .and_then(|a| a.get("time"))
        .and_then(|t| serde_json::from_value(t.clone()).ok());
    let since = last_anchor_time.unwrap_or_else(|| Utc::now() - chrono::Duration::days(14));

    let row = sqlx::query(
        r#"SELECT pattern_family, subkind, direction, anchors, end_time, raw_meta
             FROM detections
            WHERE exchange = $1 AND segment = $2 AND symbol = $3
              AND timeframe = $4 AND mode = 'live'
              AND (
                pattern_family = 'motive'
                OR (pattern_family = 'elliott_early'
                    AND subkind ~* '^impulse_forming_')
              )
              AND direction = $5
              AND end_time >= $6
            ORDER BY end_time DESC
            LIMIT 1"#,
    )
    .bind(&parent.exchange)
    .bind(&parent.segment)
    .bind(&parent.symbol)
    .bind(child_tf)
    .bind(expected_dir)
    .bind(since)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None); };

    let direction: i16 = row.try_get("direction").unwrap_or(0);
    let anchors: Value = row.try_get("anchors").unwrap_or(Value::Null);
    let end_time: DateTime<Utc> = row.try_get("end_time").unwrap_or_else(|_| Utc::now());
    let raw_meta: Value = row.try_get("raw_meta").unwrap_or(Value::Null);
    let score = raw_meta
        .get("score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.6);
    if score < min_score {
        return Ok(None);
    }

    Ok(Some(ChildMotive {
        direction: direction as i8,
        anchors,
        end_time,
        score,
    }))
}

// ─── persistence ──────────────────────────────────────────────────────

async fn write_iq_t_setup(
    pool: &PgPool,
    parent: &ParentStructure,
    child_tf: &str,
    child: &ChildMotive,
    expected_dir: i16,
) -> anyhow::Result<()> {
    // Pull entry / SL / TP from the child motive's anchors. Convention
    // mirrors the Pine port motive layout: anchors[0] = W0 start,
    // anchors[5] = W5 end. Bull motive: entry near W2 retrace, SL at
    // W0 (impulse start), TP at W5 + extension. Bear: mirror.
    let entry_price = child
        .anchors
        .as_array()
        .and_then(|a| a.get(2))                    // W2 end
        .and_then(|a| a.get("price"))
        .and_then(|p| p.as_f64())
        .unwrap_or_default();
    let sl_price = child
        .anchors
        .as_array()
        .and_then(|a| a.get(0))                    // W0 start
        .and_then(|a| a.get("price"))
        .and_then(|p| p.as_f64())
        .unwrap_or_default();
    let tp_price = child
        .anchors
        .as_array()
        .and_then(|a| a.last())                    // W5 (or last available)
        .and_then(|a| a.get("price"))
        .and_then(|p| p.as_f64())
        .unwrap_or_default();
    if entry_price <= 0.0 || sl_price <= 0.0 || tp_price <= 0.0 {
        return Ok(());
    }
    // Try to find the parent IQ-D setup so we can wire parent_setup_id.
    // If no parent setup exists yet (PR-25C not yet wiring iq_d setups),
    // leave it NULL — the IQ-T setup is still useful as a standalone
    // signal and the link can be back-filled later.
    let parent_setup_id: Option<uuid::Uuid> = sqlx::query(
        r#"SELECT id FROM qtss_setups
            WHERE profile = 'iq_d'
              AND raw_meta->>'iq_structure_id' = $1::text
            ORDER BY created_at DESC
            LIMIT 1"#,
    )
    .bind(parent.id.to_string())
    .fetch_optional(pool)
    .await?
    .and_then(|r| r.try_get("id").ok());

    let direction_str = if expected_dir == 1 { "long" } else { "short" };
    let raw_meta = json!({
        "iq_structure_id": parent.id.to_string(),
        "parent_tf": parent.timeframe,
        "parent_slot": parent.slot,
        "parent_current_wave": parent.current_wave,
        "child_tf": child_tf,
        "child_motive_end_time": child.end_time.to_rfc3339(),
        "child_motive_score": child.score,
    });
    let key = format!(
        "iq_t:{}:{}:{}:{}:{}:{}",
        parent.exchange, parent.symbol, child_tf, parent.slot, child.end_time.timestamp(), expected_dir
    );

    // Pre-check idempotency (partial unique index can't be referenced
    // via ON CONFLICT (idempotency_key) — that requires a constraint).
    let dup = sqlx::query(
        "SELECT 1 FROM qtss_setups WHERE idempotency_key = $1 LIMIT 1",
    )
    .bind(&key)
    .fetch_optional(pool)
    .await?;
    if dup.is_some() {
        return Ok(());
    }
    sqlx::query(
        r#"INSERT INTO qtss_setups
              (venue_class, exchange, symbol, timeframe, profile, state,
               direction, entry_price, entry_sl, target_ref,
               raw_meta, mode, idempotency_key, parent_setup_id)
           VALUES ('crypto', $1, $2, $3, 'iq_t', 'armed',
                   $4, $5::real, $6::real, $7::real,
                   $8, 'dry', $9, $10)"#,
    )
    .bind(&parent.exchange)
    .bind(&parent.symbol)
    .bind(child_tf)
    .bind(direction_str)
    .bind(entry_price)
    .bind(sl_price)
    .bind(tp_price)
    .bind(&raw_meta)
    .bind(&key)
    .bind(parent_setup_id)
    .execute(pool)
    .await?;
    Ok(())
}
