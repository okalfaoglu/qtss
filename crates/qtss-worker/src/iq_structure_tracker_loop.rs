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
        let Some(update) = update else { continue };
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
