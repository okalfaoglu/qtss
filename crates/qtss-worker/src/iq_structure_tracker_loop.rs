// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `iq_structure_tracker_loop` — FAZ 25 PR-25B.
//!
//! Walks the elliott + elliott_early detection rows on a tick and
//! materialises one `iq_structures` row per (exchange, segment,
//! symbol, timeframe, slot) tuple. The structure row tracks the
//! current Elliott wave (W1..W5, A..C), state (candidate / tracking
//! / completed / invalidated), and the chronological anchor list.
//!
//! Strict-isolation principle (FAZ 25 §0): this loop only WRITES to
//! `iq_structures` and `iq_symbol_locks`. The legacy T/D allocator,
//! existing setup_watcher, qtss_setups writes — all untouched.
//!
//! State machine (per (symbol, tf, slot) tuple):
//!
//!   none ── nascent_impulse hit ──> candidate (current_wave=W3, stage=nascent)
//!     │
//!     ├── forming_impulse hit ─────> tracking (W4, forming)
//!     ├── motive (full impulse) ────> tracking (W5, completed)
//!     ├── abc_nascent ─────────────> tracking (B, nascent)
//!     ├── abc_forming ─────────────> tracking (C, forming)
//!     ├── full ABC pattern row ────> completed (8-wave cycle done)
//!     └── invalidation rule fired ─> invalidated + lock symbol
//!
//! Invalidation rules (Elliott canonical):
//!   * W2 retraces > 100% of W1 (price drops below W1 start)
//!   * W4 overlaps W1 (W4 low/high crosses W1 high/low)
//!   * W3 ends up shortest of W1/W3/W5
//!   * Post-W5 high/low breaks the (5) point
//!     (the move was a sub-wave, not a true motive end)
//!
//! When invalidated, an `iq_symbol_locks` row is inserted/updated for
//! the symbol. The lock auto-clears when a NEW candidate detection
//! arrives — the tracker DELETEs the lock before creating the new
//! `iq_structures` row.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

const DEFAULT_TICK_SECS: u64 = 90;

pub async fn iq_structure_tracker_loop(pool: PgPool) {
    info!("iq_structure_tracker_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            continue;
        }
        match run_tick(&pool).await {
            Ok((scanned, advanced, locked)) => info!(
                scanned, advanced, locked,
                "iq_structure_tracker tick ok"
            ),
            Err(e) => warn!(%e, "iq_structure_tracker tick failed"),
        }
        let secs = load_tick_secs(&pool).await;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

// ─── config ───────────────────────────────────────────────────────────

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='iq_structure' AND config_key='enabled'",
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
        "SELECT value FROM system_config
           WHERE module='iq_structure' AND config_key='tick_secs'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return DEFAULT_TICK_SECS; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("secs").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_TICK_SECS).max(30)
}

async fn load_invalidation_tol(pool: &PgPool) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='iq_structure' AND config_key='invalidation_tol_pct'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.005; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("value").and_then(|v| v.as_f64()).unwrap_or(0.005)
}

/// Maximum age (in TF bars) of the latest structural anchor before we
/// flip the structure to `completed` and stop spawning new IQ-D setups.
///
/// Why this gate exists: the elliott_early detector scans full history
/// looking for 4/5-pivot impulse skeletons. Without a recency filter
/// it happily fingers a 2024 W3 peak as "nascent W3 in progress" even
/// though price retraced 60% from that peak two years ago. Operators
/// saw long ETHUSDT setups spawn at $4098 while spot was $2300 and
/// rightly flagged the regression. Default 8 bars → ~2 months on 1w,
/// ~8 days on 1d, ~32 hours on 1h. Operators tune via system_config.
async fn load_nascent_max_anchor_age_bars(pool: &PgPool) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='iq_structure' AND config_key='nascent_max_anchor_age_bars'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 8; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("value")
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        .unwrap_or(8)
        .max(1)
}

/// Convert a timeframe label (`1m`, `15m`, `1h`, `4h`, `1d`, `1w`) into
/// its bar duration in seconds. Anything we don't recognise falls back
/// to `1d` so the recency gate still trips eventually.
fn timeframe_seconds(tf: &str) -> i64 {
    let trimmed = tf.trim().to_lowercase();
    let (n, unit) = match trimmed.find(|c: char| c.is_alphabetic()) {
        Some(idx) => (&trimmed[..idx], &trimmed[idx..]),
        None => return 86_400,
    };
    let n: i64 = n.parse().unwrap_or(1);
    let secs = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 604_800,
        _ => 86_400,
    };
    n * secs
}

/// `true` when the last anchor in the structure_anchors array is older
/// than `max_age_bars × tf_seconds`. Used to gate nascent / forming
/// detections so historic skeletons don't masquerade as live setups.
fn anchor_too_stale(anchors: &Value, tf: &str, max_age_bars: i64, now: DateTime<Utc>) -> bool {
    let Some(arr) = anchors.as_array() else { return false; };
    let Some(last) = arr.last() else { return false; };
    let Some(ts_str) = last.get("time").and_then(|v| v.as_str()) else { return false; };
    let Ok(ts) = DateTime::parse_from_rfc3339(ts_str) else { return false; };
    let age = now.signed_duration_since(ts.with_timezone(&Utc));
    let max_age_secs = timeframe_seconds(tf) * max_age_bars;
    age.num_seconds() > max_age_secs
}

// ─── data shapes ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct DetectionRow {
    exchange: String,
    segment: String,
    symbol: String,
    timeframe: String,
    slot: i16,
    direction: i16,
    pattern_family: String,
    subkind: String,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    anchors: Value,
}

#[derive(Debug, Clone)]
struct StructureUpdate {
    state: String,
    current_wave: String,
    current_stage: String,
    seed_hash: String,
    structure_anchors: Value,
    invalidation_reason: Option<String>,
    raw_meta: Value,
}

// ─── FAZ 25.2.B — live projection ─────────────────────────────────────
//
// Canonical state machine = iq_structures. Detections feed it raw
// anchors; PROJECTION is computed here, every tick, against the latest
// known anchor — so the simulation tracks reality instead of locking
// into a stale future bar_index that pre-dated the current candle.
//
// User feedback (2026-04-26): "simülasyon gerçek hayattan kopuk
// ilerliyor. simülasyonda gerçekleştiğinde o artık elliot olmuştır."
// The projection structure here exists so the API/chart can render
// dotted segments that ALWAYS end at or before the latest pivot —
// promotion to "real" happens automatically when a real pivot lands
// near the projected anchor.

/// 0..1 score: 1.0 if `value` sits inside `[lo, hi]`, then linearly
/// decays to 0 over a 30% buffer outside the band. Used by FAZ 25.2.C
/// branch scoring so a B retracement that's slightly outside the
/// canonical zigzag 0.382-0.786 still gets partial credit instead of
/// a hard binary cutoff.
fn score_in_band(value: f64, lo: f64, hi: f64) -> f64 {
    if value.is_nan() || lo.is_nan() || hi.is_nan() {
        return 0.0;
    }
    if value >= lo && value <= hi {
        return 1.0;
    }
    let band_width = (hi - lo).abs().max(1e-9);
    let buffer = band_width * 0.30;
    let distance = if value < lo { lo - value } else { value - hi };
    let score = 1.0 - distance / buffer;
    score.clamp(0.0, 1.0)
}

/// Pick the highest-scoring branch with a deterministic tiebreaker
/// (left-to-right in the input). Used by the parallel-hypothesis
/// projector so when two shapes score equally we keep the more
/// "vanilla" interpretation as primary.
fn pick_primary_branch(scores: &[(&'static str, f64)]) -> &'static str {
    let mut best_kind = scores.first().map(|(k, _)| *k).unwrap_or("");
    let mut best_score = -1.0_f64;
    for (kind, score) in scores {
        if *score > best_score + 1e-9 {
            best_score = *score;
            best_kind = kind;
        }
    }
    best_kind
}

/// Project the next expected anchor for a given Elliott state.
///
/// Inputs:
/// - `current_wave` — `"W1".."W5"` or `"A"`/`"B"`/`"C"` from the state
///   machine.
/// - `direction` — +1 bull motive / -1 bear motive (the parent
///   impulse's direction; ABC direction is derived inside).
/// - `structure_anchors` — the `iq_structures.structure_anchors` JSON
///   array, each element `{wave_label, price, time, bar_index}`.
///
/// Output (JSON injected into `iq_structures.raw_meta.projection`):
/// ```json
/// {
///   "expected_next_wave": "W4",
///   "expected_direction": -1,
///   "primary": { "label": "W4?", "price": 71500.0 },
///   "alternatives": [
///       { "label": "W4? (0.382)", "price": 72100.0 },
///       { "label": "W4? (0.618)", "price": 70800.0 }
///   ],
///   "invalidation_price": 65676.0,
///   "invalidation_rule": "W4 cannot enter W1 territory"
/// }
/// ```
///
/// Returns `Value::Null` when no projection applies (e.g. completed
/// cycles).
fn compute_projection(
    current_wave: &str,
    direction: i16,
    anchors: &Value,
) -> Value {
    let arr = match anchors.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => return Value::Null,
    };
    let price_at = |label: &str| -> Option<f64> {
        arr.iter()
            .find(|a| a.get("wave_label").and_then(|v| v.as_str()) == Some(label))
            .and_then(|a| a.get("price").and_then(|p| p.as_f64()))
    };
    // Trust the price tape over the `direction` column. Pine port
    // sometimes seeds iq_structures with the W0 anchor's PIVOT direction
    // (low = -1, high = +1) rather than the motive's structural
    // direction. A bull motive starts at a LOW (W0 dir=-1) but the
    // motive itself runs upward — so reading `direction` literal would
    // flip every comparison below. Derive bull from W5>W0 instead;
    // fall back to the column only when prices are unavailable.
    let bull = match (price_at("W5"), price_at("W0")) {
        (Some(w5), Some(w0)) => w5 > w0,
        _ => direction == 1,
    };
    let dir_sign = if bull { 1.0 } else { -1.0 };

    match current_wave {
        // Nascent W3 — already see W0/W1/W2/W3. Next expected is W4
        // retrace of the W2→W3 leg. Canonical Fib: 0.382 (shallow,
        // strong-trend), 0.500 (neutral), 0.236 (very shallow).
        "W3" => {
            let w2 = price_at("W2");
            let w3 = price_at("W3");
            let w1 = price_at("W1");
            match (w1, w2, w3) {
                (Some(w1p), Some(w2p), Some(w3p)) => {
                    let leg = w3p - w2p;
                    let mid = w3p - 0.382 * leg;
                    let alt_shallow = w3p - 0.236 * leg;
                    let alt_deep = w3p - 0.500 * leg;
                    serde_json::json!({
                        "expected_next_wave": "W4",
                        "expected_direction": -direction,
                        "primary": { "label": "W4? (0.382)", "price": mid },
                        "alternatives": [
                            { "label": "W4? (0.236)", "price": alt_shallow },
                            { "label": "W4? (0.500)", "price": alt_deep }
                        ],
                        "invalidation_price": w1p,
                        "invalidation_rule": "W4 cannot enter W1 territory (Frost & Prechter rule 3)"
                    })
                }
                _ => Value::Null,
            }
        }
        // W4 forming/completed — project W5. Canonical W5 ≈ W1 length
        // from W4 (equality), or 0.618×W1 (truncated), or 1.618×W1
        // (extended).
        "W4" => {
            let w0 = price_at("W0");
            let w1 = price_at("W1");
            let w4 = price_at("W4");
            match (w0, w1, w4) {
                (Some(w0p), Some(w1p), Some(w4p)) => {
                    let w1_leg = w1p - w0p;
                    let mid = w4p + w1_leg;
                    let alt_short = w4p + 0.618 * w1_leg;
                    let alt_long = w4p + 1.618 * w1_leg;
                    serde_json::json!({
                        "expected_next_wave": "W5",
                        "expected_direction": direction,
                        "primary": { "label": "W5? (1.0×W1)", "price": mid },
                        "alternatives": [
                            { "label": "W5? (0.618×W1)", "price": alt_short },
                            { "label": "W5? (1.618×W1)", "price": alt_long }
                        ],
                        "invalidation_price": w4p,
                        "invalidation_rule": "W5 must close beyond W3 (else truncated fifth)"
                    })
                }
                _ => Value::Null,
            }
        }
        // W5 completed — corrective ABC starts. Project A first (50%
        // retrace of W0→W5 baseline). B and C come later when more
        // pivots arrive.
        "W5" => {
            let w0 = price_at("W0");
            let w5 = price_at("W5");
            match (w0, w5) {
                (Some(w0p), Some(w5p)) => {
                    let leg = w5p - w0p;
                    let a_target = w5p - 0.5 * leg;
                    serde_json::json!({
                        "expected_next_wave": "A",
                        "expected_direction": -direction,
                        "primary": { "label": "A? (0.5×W0-W5)", "price": a_target },
                        "alternatives": [
                            { "label": "A? (0.382)", "price": w5p - 0.382 * leg },
                            { "label": "A? (0.618)", "price": w5p - 0.618 * leg }
                        ],
                        "invalidation_price": w5p,
                        "invalidation_rule": "Post-W5 break above W5 invalidates zigzag corrective (flat allowed)"
                    })
                }
                _ => Value::Null,
            }
        }
        // A complete — project B. Zigzag: 0.382-0.786 retrace of A.
        // Flat: ~A length. Both shown as alternatives.
        "A" => {
            let w5 = price_at("W5");
            let a = price_at("A");
            match (w5, a) {
                (Some(w5p), Some(ap)) => {
                    let a_leg = ap - w5p;
                    let zz_target = ap - 0.5 * a_leg;
                    let flat_target = w5p; // B touches A's start (flat regular)
                    serde_json::json!({
                        "expected_next_wave": "B",
                        "expected_direction": -dir_sign as i32 * -1, // B opposes A direction
                        "primary": { "label": "B? (zigzag 0.5)", "price": zz_target },
                        "alternatives": [
                            { "label": "B? (zigzag 0.382)", "price": ap - 0.382 * a_leg },
                            { "label": "B? (zigzag 0.786)", "price": ap - 0.786 * a_leg },
                            { "label": "B? (flat regular)", "price": flat_target },
                            { "label": "B? (flat expanded 1.272)", "price": w5p + 0.272 * a_leg.abs() * (-dir_sign) }
                        ],
                        "invalidation_price": ap,
                        "invalidation_rule": "B exceeding A by >138% suggests running flat (still valid corrective)"
                    })
                }
                _ => Value::Null,
            }
        }
        // B complete — project C. FAZ 25.2.C: this state has enough
        // observed pivots (W5+A+B) to PARALLEL-RANK the corrective
        // shape hypotheses. Each branch scores 0..1 against the
        // canonical Fib bands; the highest-scoring becomes
        // `primary_branch` and the chart renders it prominently with
        // alternates faint underneath. User feedback: "Z5'in 5'i =
        // Z4'ün b'si ama 5'in üstünde, ABC olmuyor mu?" — answered
        // mechanically here: zigzag scores low when B exceeds W5
        // (= A's start), flat_expanded scores high.
        "B" => {
            let w5 = price_at("W5");
            let a = price_at("A");
            let b = price_at("B");
            match (w5, a, b) {
                (Some(w5p), Some(ap), Some(bp)) => {
                    let a_len = (ap - w5p).abs();
                    let b_ret = (bp - ap).abs();
                    let b_to_a = if a_len > 0.0 { b_ret / a_len } else { 0.0 };
                    // Did B exceed A's START (= W5)? For bull motive
                    // (W5 high), B exceeds means bp > w5p. For bear,
                    // bp < w5p.
                    let b_exceeds_w5 = if dir_sign > 0.0 {
                        bp > w5p
                    } else {
                        bp < w5p
                    };
                    // Pure ratio-based scoring per Frost-Prechter §2.5.
                    // Each branch returns a 0..1 confidence given the
                    // observed b_to_a ratio + b_exceeds_w5 flag.
                    let score_zigzag = if b_exceeds_w5 {
                        0.0
                    } else {
                        score_in_band(b_to_a, 0.382, 0.786)
                    };
                    let score_flat_regular = if b_exceeds_w5 {
                        // tiny overshoot OK (slop), but not far above
                        score_in_band(b_to_a, 0.95, 1.05)
                    } else {
                        score_in_band(b_to_a, 0.90, 1.05)
                    };
                    let score_flat_expanded = if b_exceeds_w5 {
                        score_in_band(b_to_a, 1.05, 1.382)
                    } else {
                        0.0
                    };
                    let score_flat_running = if b_exceeds_w5 {
                        score_in_band(b_to_a, 1.382, 2.5)
                    } else {
                        0.0
                    };
                    // C direction = same as A (opposite to B).
                    let c_sign = if ap < w5p { -1.0 } else { 1.0 };
                    let c_equality = bp + c_sign * a_len;
                    let c_1272 = bp + c_sign * a_len * 1.272;
                    let c_1618 = bp + c_sign * a_len * 1.618;
                    let branches = serde_json::json!([
                        {
                            "kind": "zigzag",
                            "score": score_zigzag,
                            "c_target": c_equality,
                            "c_alt_1272": c_1272,
                            "c_alt_1618": c_1618,
                            "rule": "B retraces 38.2-78.6% of A, never exceeds W5"
                        },
                        {
                            "kind": "flat_regular",
                            "score": score_flat_regular,
                            "c_target": w5p + c_sign * a_len * 0.0, // C ≈ A's start
                            "c_alt_min": w5p + c_sign * a_len * 0.0,
                            "c_alt_extended": bp + c_sign * a_len,
                            "rule": "B ≈ A length; C lands near W5 level"
                        },
                        {
                            "kind": "flat_expanded",
                            "score": score_flat_expanded,
                            "c_target": bp + c_sign * a_len * 1.272,
                            "c_alt_min": bp + c_sign * a_len,
                            "c_alt_max": bp + c_sign * a_len * 1.618,
                            "rule": "B exceeds W5 by 5-38%; C extends past A by 1.272-1.618"
                        },
                        {
                            "kind": "flat_running",
                            "score": score_flat_running,
                            "c_target": bp + c_sign * a_len * 0.618,
                            "c_alt_min": bp + c_sign * a_len * 0.382,
                            "c_alt_max": bp + c_sign * a_len,
                            "rule": "B exceeds W5 by 38%+; C truncated, fails to reach A"
                        }
                    ]);
                    // Primary = max-score branch. Tie-breaker: prefer
                    // the more "common" shape (zigzag > flat_regular
                    // > flat_expanded > flat_running).
                    let primary_branch = pick_primary_branch(&[
                        ("zigzag", score_zigzag),
                        ("flat_regular", score_flat_regular),
                        ("flat_expanded", score_flat_expanded),
                        ("flat_running", score_flat_running),
                    ]);
                    serde_json::json!({
                        "expected_next_wave": "C",
                        "expected_direction": (c_sign as i32),
                        "primary": { "label": "C? (=A)", "price": c_equality },
                        "alternatives": [
                            { "label": "C? (1.272×A)", "price": c_1272 },
                            { "label": "C? (1.618×A)", "price": c_1618 }
                        ],
                        "invalidation_price": bp,
                        "invalidation_rule": "C must clear A end for valid zigzag/flat completion",
                        "branches": branches,
                        "primary_branch": primary_branch,
                        "observed_b_to_a_ratio": b_to_a,
                        "b_exceeds_w5": b_exceeds_w5
                    })
                }
                _ => Value::Null,
            }
        }
        // C complete — corrective cycle done; the next move is a new
        // impulse. Don't project further at this layer.
        "C" => Value::Null,
        _ => Value::Null,
    }
}

// ─── tick body ────────────────────────────────────────────────────────

async fn run_tick(pool: &PgPool) -> anyhow::Result<(usize, usize, usize)> {
    let tol = load_invalidation_tol(pool).await;
    let max_age_bars = load_nascent_max_anchor_age_bars(pool).await;
    let now = Utc::now();

    // Staleness sweep: any structure whose latest anchor is older than
    // the configured horizon is flagged `completed` so the IQ-D
    // candidate loop stops emitting setups for it. Runs ahead of the
    // normal state-machine pass so a freshly invalidated row still
    // gets re-scanned this tick.
    let stale_count = mark_stale_structures_completed(pool, max_age_bars, now).await?;
    if stale_count > 0 {
        info!(stale_count, "iq_structure_tracker: marked stale structures completed");
    }

    // Pull every "interesting" detection row from the last 24h. The
    // tracker only cares about the LATEST row per (symbol, tf, slot,
    // family) so we reduce in-memory after fetch — keeps the SQL
    // simple and resilient to schema additions.
    let rows = sqlx::query(
        r#"SELECT exchange, segment, symbol, timeframe, slot, direction,
                  pattern_family, subkind, start_time, end_time, anchors
             FROM detections
            WHERE mode = 'live'
              AND (
                pattern_family IN ('motive', 'abc') OR
                (pattern_family = 'elliott_early' AND subkind ~* '^(impulse_(nascent|forming)|abc_(nascent|forming))_')
              )
              AND detected_at > now() - interval '24 hours'
            ORDER BY end_time DESC
            LIMIT 5000"#,
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok((0, 0, 0));
    }

    // Bucket by (exchange, segment, symbol, tf, slot) → latest row per
    // family. The "latest of each family" snapshot drives the state
    // machine.
    use std::collections::BTreeMap;
    type Key = (String, String, String, String, i16);
    let mut by_key: BTreeMap<Key, Vec<DetectionRow>> = BTreeMap::new();
    for r in rows {
        let row = DetectionRow {
            exchange: r.try_get("exchange").unwrap_or_default(),
            segment: r.try_get("segment").unwrap_or_default(),
            symbol: r.try_get("symbol").unwrap_or_default(),
            timeframe: r.try_get("timeframe").unwrap_or_default(),
            slot: r.try_get("slot").unwrap_or(0),
            direction: r.try_get("direction").unwrap_or(0),
            pattern_family: r.try_get("pattern_family").unwrap_or_default(),
            subkind: r.try_get("subkind").unwrap_or_default(),
            start_time: r.try_get("start_time").unwrap_or_else(|_| Utc::now()),
            end_time: r.try_get("end_time").unwrap_or_else(|_| Utc::now()),
            anchors: r.try_get("anchors").unwrap_or(Value::Null),
        };
        let key: Key = (
            row.exchange.clone(),
            row.segment.clone(),
            row.symbol.clone(),
            row.timeframe.clone(),
            row.slot,
        );
        by_key.entry(key).or_default().push(row);
    }

    let mut scanned = 0usize;
    let mut advanced = 0usize;
    let mut locked = 0usize;
    for (key, group) in by_key {
        scanned += 1;
        let tf = key.3.as_str();
        let update = derive_update(&group, tol, tf, max_age_bars, now);
        let Some(mut update) = update else { continue };

        // FAZ 25.2.B — every state transition gets a freshly-computed
        // projection injected into raw_meta. Caller (chart, allocator)
        // reads `iq_structures.raw_meta.projection` as the canonical
        // forward-look. Anchor list itself stays REAL pivots only.
        let dir = group.first().map(|r| r.direction).unwrap_or(0);
        let proj = compute_projection(
            &update.current_wave,
            dir,
            &update.structure_anchors,
        );
        if !proj.is_null() {
            if let Value::Object(map) = &mut update.raw_meta {
                map.insert("projection".to_string(), proj);
            } else {
                update.raw_meta = json!({
                    "projection": proj,
                    "previous": update.raw_meta.clone(),
                });
            }
        }
        match upsert_structure(pool, &key, &group[0], &update).await {
            Ok(was_advance) => {
                if was_advance {
                    advanced += 1;
                }
                if update.state == "invalidated" {
                    if let Err(e) = lock_symbol(pool, &key, &update).await {
                        warn!(%e, symbol=%key.2, "iq_structure lock_symbol failed");
                    } else {
                        locked += 1;
                    }
                }
                if update.state == "candidate" {
                    // A fresh candidate clears any pre-existing lock
                    // on the symbol.
                    let _ = unlock_symbol(pool, &key).await;
                }
                if was_advance {
                    // Push to SSE subscribers so the chart picks up
                    // structure transitions (e.g. W3 → W4 → W5)
                    // within a second instead of waiting for the
                    // 30s polling tick.
                    let _ = sqlx::query("SELECT pg_notify('qtss_iq_changed', $1)")
                        .bind(json!({
                            "kind": "iq_structure",
                            "exchange": key.0,
                            "segment": key.1,
                            "symbol": key.2,
                            "timeframe": key.3,
                            "slot": key.4,
                            "state": update.state,
                            "current_wave": update.current_wave,
                        }).to_string())
                        .execute(pool)
                        .await;
                }
            }
            Err(e) => warn!(%e, symbol=%key.2, "iq_structure upsert failed"),
        }
    }

    Ok((scanned, advanced, locked))
}

// ─── state machine ────────────────────────────────────────────────────

/// Looks at the latest set of detection rows for a single
/// (exchange, segment, symbol, tf, slot) tuple and returns the
/// structure update we should apply. `None` means "nothing actionable
/// in this group" — typically too few rows or all stale.
fn derive_update(
    group: &[DetectionRow],
    tol_pct: f64,
    tf: &str,
    max_age_bars: i64,
    now: DateTime<Utc>,
) -> Option<StructureUpdate> {
    // Pick the freshest row for each family / sub-stage.
    let mut latest_motive: Option<&DetectionRow> = None;
    let mut latest_abc: Option<&DetectionRow> = None;
    let mut latest_nascent: Option<&DetectionRow> = None;
    let mut latest_forming: Option<&DetectionRow> = None;
    let mut latest_abc_nascent: Option<&DetectionRow> = None;
    let mut latest_abc_forming: Option<&DetectionRow> = None;
    fn pick_latest<'a>(slot: &mut Option<&'a DetectionRow>, candidate: &'a DetectionRow) {
        match *slot {
            Some(prev) if prev.end_time >= candidate.end_time => {}
            _ => *slot = Some(candidate),
        }
    }
    for r in group {
        let pf = r.pattern_family.as_str();
        let sk = r.subkind.as_str();
        match (pf, sk) {
            ("motive", _) => pick_latest(&mut latest_motive, r),
            ("abc", _) => pick_latest(&mut latest_abc, r),
            ("elliott_early", s) if s.starts_with("impulse_nascent") => {
                pick_latest(&mut latest_nascent, r)
            }
            ("elliott_early", s) if s.starts_with("impulse_forming") => {
                pick_latest(&mut latest_forming, r)
            }
            ("elliott_early", s) if s.starts_with("abc_nascent") => {
                pick_latest(&mut latest_abc_nascent, r)
            }
            ("elliott_early", s) if s.starts_with("abc_forming") => {
                pick_latest(&mut latest_abc_forming, r)
            }
            _ => {}
        }
    }

    // Decide the freshest stage. Order: full ABC > abc_forming >
    // abc_nascent > full motive > forming impulse > nascent impulse.
    if let Some(abc) = latest_abc {
        // Full 8-wave cycle complete.
        let anchors = build_anchors_from_motive_abc(latest_motive, abc);
        return Some(StructureUpdate {
            state: "completed".into(),
            current_wave: "C".into(),
            current_stage: "completed".into(),
            seed_hash: seed_hash_from_anchors(&anchors),
            structure_anchors: anchors,
            invalidation_reason: None,
            raw_meta: json!({"source": "abc"}),
        });
    }
    if let Some(abcf) = latest_abc_forming {
        let anchors = combine_motive_with_abc_partial(latest_motive, abcf, "C", true);
        return Some(StructureUpdate {
            state: "tracking".into(),
            current_wave: "C".into(),
            current_stage: "forming".into(),
            seed_hash: seed_hash_from_anchors(&anchors),
            structure_anchors: anchors,
            invalidation_reason: None,
            raw_meta: json!({"source": "abc_forming"}),
        });
    }
    if let Some(abcn) = latest_abc_nascent {
        let anchors = combine_motive_with_abc_partial(latest_motive, abcn, "B", false);
        return Some(StructureUpdate {
            state: "tracking".into(),
            current_wave: "B".into(),
            current_stage: "forming".into(),
            seed_hash: seed_hash_from_anchors(&anchors),
            structure_anchors: anchors,
            invalidation_reason: None,
            raw_meta: json!({"source": "abc_nascent"}),
        });
    }
    if let Some(m) = latest_motive {
        // Full impulse complete; ABC not yet started.
        let invalid = check_motive_invalidation(m, tol_pct);
        let (state, reason) = if let Some(why) = invalid {
            ("invalidated".into(), Some(why))
        } else {
            ("tracking".to_string(), None)
        };
        let anchors = motive_anchor_array(m);
        return Some(StructureUpdate {
            state,
            current_wave: "W5".into(),
            current_stage: "completed".into(),
            seed_hash: seed_hash_from_anchors(&anchors),
            structure_anchors: anchors,
            invalidation_reason: reason,
            raw_meta: json!({"source": "motive"}),
        });
    }
    if let Some(f) = latest_forming {
        let anchors = early_anchor_array(f);
        // Staleness gate: forming impulse whose W4 anchor is from
        // months ago is structurally complete by now — flip to
        // `completed` so IQ-D doesn't open a W3-tier setup at a
        // long-gone wave peak. (User report: ETHUSDT 1w long opened
        // at $4098 while spot was $2300 because the W3 anchor was 2
        // years old.)
        if anchor_too_stale(&anchors, tf, max_age_bars, now) {
            return Some(StructureUpdate {
                state: "completed".into(),
                current_wave: "W5".into(),
                current_stage: "completed".into(),
                seed_hash: seed_hash_from_anchors(&anchors),
                structure_anchors: anchors,
                invalidation_reason: Some("anchor too stale (forming)".into()),
                raw_meta: json!({"source": "impulse_forming", "stale": true}),
            });
        }
        return Some(StructureUpdate {
            state: "tracking".into(),
            current_wave: "W5".into(),
            current_stage: "forming".into(),
            seed_hash: seed_hash_from_anchors(&anchors),
            structure_anchors: anchors,
            invalidation_reason: None,
            raw_meta: json!({"source": "impulse_forming"}),
        });
    }
    if let Some(n) = latest_nascent {
        let anchors = early_anchor_array(n);
        // Staleness gate (same rationale as the forming branch above):
        // a nascent W3 whose W3 anchor is older than the configured
        // horizon means the move already happened and very likely
        // reversed. We mark it `completed` so the candidate loop's
        // `state IN ('candidate','tracking')` filter excludes it.
        if anchor_too_stale(&anchors, tf, max_age_bars, now) {
            return Some(StructureUpdate {
                state: "completed".into(),
                current_wave: "W3".into(),
                current_stage: "completed".into(),
                seed_hash: seed_hash_from_anchors(&anchors),
                structure_anchors: anchors,
                invalidation_reason: Some("anchor too stale (nascent)".into()),
                raw_meta: json!({"source": "impulse_nascent", "stale": true}),
            });
        }
        return Some(StructureUpdate {
            state: "candidate".into(),
            current_wave: "W3".into(),
            current_stage: "nascent".into(),
            seed_hash: seed_hash_from_anchors(&anchors),
            structure_anchors: anchors,
            invalidation_reason: None,
            raw_meta: json!({"source": "impulse_nascent"}),
        });
    }
    None
}

/// Fast path for sweeping already-stored structures to `completed`
/// without waiting for a fresh detection row to retrigger
/// `derive_update`. Runs once per tick. Picks any candidate /
/// tracking row whose last `structure_anchors[].time` falls outside
/// the recency window for its timeframe.
async fn mark_stale_structures_completed(
    pool: &PgPool,
    max_age_bars: i64,
    now: DateTime<Utc>,
) -> anyhow::Result<usize> {
    let rows = sqlx::query(
        r#"SELECT id, timeframe, structure_anchors
             FROM iq_structures
            WHERE state IN ('candidate','tracking')"#,
    )
    .fetch_all(pool)
    .await?;
    let mut count = 0usize;
    for r in rows {
        let id: uuid::Uuid = r.try_get("id")?;
        let tf: String = r.try_get("timeframe").unwrap_or_default();
        let anchors: Value = r.try_get("structure_anchors").unwrap_or(Value::Null);
        if !anchor_too_stale(&anchors, &tf, max_age_bars, now) {
            continue;
        }
        sqlx::query(
            r#"UPDATE iq_structures
                  SET state = 'completed',
                      current_stage = 'completed',
                      completed_at = COALESCE(completed_at, now()),
                      invalidation_reason = COALESCE(invalidation_reason,
                                              'anchor too stale (sweep)'),
                      raw_meta = raw_meta || jsonb_build_object('stale_sweep', true),
                      updated_at = now()
                WHERE id = $1"#,
        )
        .bind(id)
        .execute(pool)
        .await?;
        count += 1;
    }
    Ok(count)
}

fn motive_anchor_array(m: &DetectionRow) -> Value {
    let labels = ["W0", "W1", "W2", "W3", "W4", "W5"];
    enrich_anchors(&m.anchors, &labels)
}

fn early_anchor_array(d: &DetectionRow) -> Value {
    // Nascent: 4 anchors = W0, W1, W2, W3.
    // Forming: 5 anchors = W0, W1, W2, W3, W4.
    let labels = ["W0", "W1", "W2", "W3", "W4", "W5"];
    enrich_anchors(&d.anchors, &labels)
}

fn combine_motive_with_abc_partial(
    motive: Option<&DetectionRow>,
    abc_partial: &DetectionRow,
    current_wave: &str,
    forming: bool,
) -> Value {
    let mut out: Vec<Value> = Vec::new();
    if let Some(m) = motive {
        let labelled = motive_anchor_array(m);
        if let Some(arr) = labelled.as_array() {
            out.extend(arr.iter().cloned());
        }
    }
    let abc_labels = match current_wave {
        "B" => &["W5", "A", "B"][..],
        "C" => &["W5", "A", "B", "C"][..],
        _ => &["W5", "A", "B", "C"][..],
    };
    let labelled = enrich_anchors(&abc_partial.anchors, abc_labels);
    if let Some(arr) = labelled.as_array() {
        // skip the first anchor (W5) — already in motive output
        for (i, v) in arr.iter().enumerate() {
            if i == 0 && !out.is_empty() {
                continue;
            }
            out.push(v.clone());
        }
    }
    let _ = forming; // currently unused, reserved for future flag in raw_meta
    Value::Array(out)
}

fn build_anchors_from_motive_abc(motive: Option<&DetectionRow>, abc: &DetectionRow) -> Value {
    combine_motive_with_abc_partial(motive, abc, "C", false)
}

fn enrich_anchors(anchors: &Value, labels: &[&str]) -> Value {
    let Some(arr) = anchors.as_array() else { return Value::Array(vec![]); };
    let mut out = Vec::with_capacity(arr.len());
    for (i, a) in arr.iter().enumerate() {
        let mut obj = a.clone();
        let label = labels.get(i).copied().unwrap_or("?");
        if let Some(map) = obj.as_object_mut() {
            map.insert("wave_label".into(), Value::String(label.to_string()));
        }
        out.push(obj);
    }
    Value::Array(out)
}

fn seed_hash_from_anchors(anchors: &Value) -> String {
    let arr = match anchors.as_array() {
        Some(a) => a,
        None => return "empty".into(),
    };
    if arr.is_empty() {
        return "empty".into();
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    for a in arr.iter().take(3) {
        let p = a.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
        // Round to 4 sig figs so tiny float drift doesn't produce a
        // different hash for the same logical structure.
        let rounded = (p * 10_000.0).round() as i64;
        rounded.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

/// Apply the canonical Elliott invalidation rules to a complete
/// motive. Returns the human-readable reason on failure, `None` when
/// the motive is still valid.
fn check_motive_invalidation(m: &DetectionRow, tol_pct: f64) -> Option<String> {
    let Some(anchors) = m.anchors.as_array() else { return None; };
    if anchors.len() < 6 {
        return None;
    }
    let prices: Vec<f64> = anchors
        .iter()
        .map(|a| a.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0))
        .collect();
    if prices.iter().any(|p| *p <= 0.0) {
        return None;
    }
    // Normalise to a bullish frame for rule checks. dir = +1 => no
    // sign flip; dir = -1 => negate prices so all rules read the
    // same "going up" frame.
    let dir = m.direction as f64;
    let p: Vec<f64> = prices.iter().map(|x| x * dir).collect();
    let tol = tol_pct.abs();
    // Rule 1: W2 retraces > 100% of W1 (p2 below p0 in normalised frame).
    if p[2] < p[0] * (1.0 - tol) {
        return Some("W2 retraces beyond W1 start".into());
    }
    // Rule 2: W4 overlaps W1 territory (p4 < p1).
    if p[4] < p[1] * (1.0 - tol) {
        return Some("W4 overlaps W1".into());
    }
    // Rule 3: W3 not the shortest of W1/W3/W5.
    let w1 = (p[1] - p[0]).abs();
    let w3 = (p[3] - p[2]).abs();
    let w5 = (p[5] - p[4]).abs();
    if w3 < w1 && w3 < w5 {
        return Some("W3 is the shortest of W1/W3/W5".into());
    }
    None
}

// ─── persistence ──────────────────────────────────────────────────────

/// Insert or update the iq_structures row for this key. Returns
/// `true` when the structure actually advanced (new wave/stage or
/// state change), `false` when nothing changed.
async fn upsert_structure(
    pool: &PgPool,
    key: &(String, String, String, String, i16),
    sample: &DetectionRow,
    update: &StructureUpdate,
) -> anyhow::Result<bool> {
    let (exchange, segment, symbol, timeframe, slot) = key;

    // First: see if there's an active row (state IN candidate/tracking)
    // for this tuple. If yes, transition it. If not, create a new row.
    let active = sqlx::query(
        r#"SELECT id, state, current_wave, current_stage, seed_hash
             FROM iq_structures
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4 AND slot=$5
              AND state IN ('candidate','tracking')
            LIMIT 1"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .bind(slot)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = active {
        let id: uuid::Uuid = row.try_get("id")?;
        let prev_state: String = row.try_get("state").unwrap_or_default();
        let prev_wave: String = row.try_get("current_wave").unwrap_or_default();
        let prev_stage: String = row.try_get("current_stage").unwrap_or_default();
        let prev_seed: String = row.try_get("seed_hash").unwrap_or_default();
        let advanced = prev_state != update.state
            || prev_wave != update.current_wave
            || prev_stage != update.current_stage
            || prev_seed != update.seed_hash;

        // If the seed hash changed and the previous structure wasn't
        // already terminal, mark the OLD one invalidated (replaced)
        // so we keep an audit trail rather than silently mutating.
        if prev_seed != update.seed_hash {
            sqlx::query(
                r#"UPDATE iq_structures
                       SET state='invalidated',
                           invalidated_at = COALESCE(invalidated_at, now()),
                           invalidation_reason = 'replaced by fresher seed',
                           updated_at = now()
                     WHERE id = $1"#,
            )
            .bind(id)
            .execute(pool)
            .await?;
            // Insert a new row for the fresh seed.
            insert_fresh(pool, key, sample, update).await?;
            return Ok(true);
        }

        sqlx::query(
            r#"UPDATE iq_structures
                   SET state = $2,
                       current_wave = $3,
                       current_stage = $4,
                       structure_anchors = $5,
                       invalidation_reason = $6,
                       invalidated_at = CASE
                                          WHEN $2 = 'invalidated' THEN COALESCE(invalidated_at, now())
                                          ELSE invalidated_at
                                        END,
                       completed_at = CASE
                                        WHEN $2 = 'completed' THEN COALESCE(completed_at, now())
                                        ELSE completed_at
                                      END,
                       last_advanced_at = CASE
                                            WHEN $7 THEN now()
                                            ELSE last_advanced_at
                                          END,
                       raw_meta = $8,
                       updated_at = now()
                 WHERE id = $1"#,
        )
        .bind(id)
        .bind(&update.state)
        .bind(&update.current_wave)
        .bind(&update.current_stage)
        .bind(&update.structure_anchors)
        .bind(&update.invalidation_reason)
        .bind(advanced)
        .bind(&update.raw_meta)
        .execute(pool)
        .await?;
        // Mirror to the snapshot table on every advance. Reading by
        // bar_time + ON CONFLICT(bar_time) means re-runs on the same
        // bar are idempotent — the captured_at timestamp is the only
        // thing that bumps.
        if advanced {
            write_structure_snapshot(pool, key, sample, update).await?;
        }
        Ok(advanced)
    } else {
        insert_fresh(pool, key, sample, update).await?;
        Ok(true)
    }
}

async fn insert_fresh(
    pool: &PgPool,
    key: &(String, String, String, String, i16),
    sample: &DetectionRow,
    update: &StructureUpdate,
) -> anyhow::Result<()> {
    let (exchange, segment, symbol, timeframe, slot) = key;
    sqlx::query(
        r#"INSERT INTO iq_structures
              (exchange, segment, symbol, timeframe, slot, direction,
               state, current_wave, current_stage,
               structure_anchors, seed_hash,
               invalidation_reason, raw_meta)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .bind(slot)
    .bind(sample.direction)
    .bind(&update.state)
    .bind(&update.current_wave)
    .bind(&update.current_stage)
    .bind(&update.structure_anchors)
    .bind(&update.seed_hash)
    .bind(&update.invalidation_reason)
    .bind(&update.raw_meta)
    .execute(pool)
    .await?;
    // Time-series snapshot — every fresh insert is also a state
    // observation at the sample bar's end_time. The backtest's
    // structural_completion scorer reads from this table to
    // reconstruct historical state without depending on the
    // point-in-time iq_structures row (which gets overwritten as
    // structures advance).
    write_structure_snapshot(pool, key, sample, update).await?;
    Ok(())
}

/// Time-series mirror of iq_structures. Captures the (state,
/// current_wave, raw_meta) tuple at `sample.end_time` so the
/// backtest can ask "what did the structure look like at bar T?"
/// without losing history when the live worker advances the
/// underlying structure row.
async fn write_structure_snapshot(
    pool: &PgPool,
    key: &(String, String, String, String, i16),
    sample: &DetectionRow,
    update: &StructureUpdate,
) -> anyhow::Result<()> {
    let (exchange, segment, symbol, timeframe, slot) = key;
    sqlx::query(
        r#"INSERT INTO iq_structure_snapshots
              (exchange, segment, symbol, timeframe, slot,
               bar_time, state, current_wave, direction, raw_meta)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
           ON CONFLICT (exchange, segment, symbol, timeframe, slot, bar_time)
           DO UPDATE SET
               state        = EXCLUDED.state,
               current_wave = EXCLUDED.current_wave,
               direction    = EXCLUDED.direction,
               raw_meta     = EXCLUDED.raw_meta,
               captured_at  = now()"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .bind(*slot)
    .bind(sample.end_time)
    .bind(&update.state)
    .bind(&update.current_wave)
    .bind(sample.direction)
    .bind(&update.raw_meta)
    .execute(pool)
    .await?;
    Ok(())
}

async fn lock_symbol(
    pool: &PgPool,
    key: &(String, String, String, String, i16),
    update: &StructureUpdate,
) -> anyhow::Result<()> {
    let (exchange, segment, symbol, _, _) = key;
    let reason = update
        .invalidation_reason
        .clone()
        .unwrap_or_else(|| "structure invalidated".into());
    sqlx::query(
        r#"INSERT INTO iq_symbol_locks (exchange, segment, symbol, reason)
           VALUES ($1,$2,$3,$4)
           ON CONFLICT (exchange, segment, symbol) DO UPDATE
              SET locked_at = now(),
                  reason    = EXCLUDED.reason"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(&reason)
    .execute(pool)
    .await?;
    debug!(symbol=%symbol, %reason, "iq_symbol_locks: locked");
    Ok(())
}

async fn unlock_symbol(
    pool: &PgPool,
    key: &(String, String, String, String, i16),
) -> anyhow::Result<()> {
    let (exchange, segment, symbol, _, _) = key;
    sqlx::query(
        "DELETE FROM iq_symbol_locks
           WHERE exchange=$1 AND segment=$2 AND symbol=$3",
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .execute(pool)
    .await?;
    Ok(())
}
