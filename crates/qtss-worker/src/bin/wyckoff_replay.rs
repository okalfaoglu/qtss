//! `qtss-wyckoff-replay` — Rebuild `wyckoff_structures` from existing
//! `qtss_v2_detections` rows.
//!
//! Why:
//!   The tracker wiring in `upsert_wyckoff_structure_from_detection`
//!   was added after the progressive historical scan had already
//!   inserted thousands of Wyckoff detection rows. Those rows never
//!   reached the tracker, so the `wyckoff_structures` table only
//!   contains a handful of Phase-A orphans. We need to replay every
//!   historical detection in chronological order to produce true
//!   A → B → C → D → E cycles (some completed, some still active).
//!
//! How:
//!   For each (exchange, symbol, interval) we walk the wyckoff
//!   detections ordered by last-anchor bar_index. We run the same
//!   tracker used by the live orchestrator:
//!     * A Phase-A event (PS/SC/BC/AR/ST) spawns a new structure.
//!     * Subsequent events advance / mutate the active structure.
//!     * A schematic flip → mark current FAILED, open fresh if the
//!       flipping event is itself a Phase A seed; otherwise drop.
//!     * Phase E reached → mark COMPLETED.
//!     * If the tracker is in Phase D at the end of history and the
//!       last ~30 bar closes show a sustained breakout consistent
//!       with the schematic's bias, inject a synthetic Markup /
//!       Markdown event (matches the live worker's P9 behaviour).
//!
//! Usage:
//!   cargo run -p qtss-worker --bin qtss-wyckoff-replay
//!   cargo run -p qtss-worker --bin qtss-wyckoff-replay -- --dry-run
//!
//! This is one-shot: run it after major logic changes, then let the
//! live orchestrator + progressive scan keep the table current.

use chrono::{DateTime, Utc};
use qtss_common::{load_dotenv, postgres_url_from_env_or_default};
use qtss_storage::{
    complete_wyckoff_structure, create_pool, fail_wyckoff_structure, insert_wyckoff_structure,
    list_recent_bars, update_wyckoff_structure, WyckoffStructureInsert,
};
use qtss_wyckoff::{
    WyckoffEvent, WyckoffPhase, WyckoffSchematic, WyckoffStructureTracker,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[allow(dead_code)]
#[derive(Debug, sqlx::FromRow)]
struct DetRow {
    exchange: String,
    symbol: String,
    timeframe: String,
    subkind: String,
    structural_score: f32,
    anchors: serde_json::Value,
    detected_at: DateTime<Utc>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv();
    let dry_run = std::env::args().any(|a| a == "--dry-run");

    println!("\n{BOLD}{CYAN}╔════════════════════════════════════════════╗{RESET}");
    println!("{BOLD}{CYAN}║   QTSS WYCKOFF STRUCTURE REPLAY (P11)      ║{RESET}");
    println!("{BOLD}{CYAN}╚════════════════════════════════════════════╝{RESET}\n");
    if dry_run {
        println!("{YELLOW}[DRY-RUN] writing disabled — only logging{RESET}\n");
    }

    let db_url = postgres_url_from_env_or_default("");
    let pool = create_pool(&db_url, 4).await?;

    // Groups to replay.
    let groups: Vec<(String, String, String)> = sqlx::query_as(
        r#"SELECT DISTINCT exchange, symbol, timeframe
             FROM qtss_v2_detections
            WHERE family = 'wyckoff'
            ORDER BY exchange, symbol, timeframe"#,
    )
    .fetch_all(&pool)
    .await?;

    if !dry_run {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM wyckoff_structures")
            .fetch_one(&pool).await.unwrap_or(0);
        println!("  purging existing wyckoff_structures ({n} rows)...");
        sqlx::query("DELETE FROM wyckoff_structures").execute(&pool).await?;
    }

    let mut total_completed = 0u64;
    let mut total_failed = 0u64;
    let mut total_active = 0u64;

    for (exchange, symbol, timeframe) in &groups {
        let (c, f, a) = replay_group(&pool, exchange, symbol, timeframe, dry_run).await?;
        total_completed += c;
        total_failed    += f;
        total_active    += a;
    }

    println!("\n{BOLD}{GREEN}replay done{RESET}");
    println!("  completed: {total_completed}");
    println!("  failed:    {total_failed}");
    println!("  active:    {total_active}\n");
    Ok(())
}

async fn replay_group(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    dry_run: bool,
) -> anyhow::Result<(u64, u64, u64)> {
    // Detections ordered as they would have fired historically. We use
    // detected_at as the primary key: detection rows are appended in
    // time order by both live orchestrator and progressive scan so this
    // is stable and faithful to the sequence the live tracker would
    // have seen.
    let dets: Vec<DetRow> = sqlx::query_as(
        r#"SELECT exchange, symbol, timeframe, subkind,
                  structural_score, anchors, detected_at
             FROM qtss_v2_detections
            WHERE family = 'wyckoff'
              AND exchange = $1 AND symbol = $2 AND timeframe = $3
            ORDER BY detected_at ASC, id ASC"#,
    )
    .bind(exchange).bind(symbol).bind(timeframe)
    .fetch_all(pool).await?;

    if dets.is_empty() {
        return Ok((0, 0, 0));
    }

    // Active state: (uuid, tracker, running range min/max).
    struct Active {
        id: Uuid,
        tracker: WyckoffStructureTracker,
        range_hi: f64,
        range_lo: f64,
    }
    let mut active: Option<Active> = None;
    let mut completed = 0u64;
    let mut failed = 0u64;

    for det in &dets {
        let (event_name, variant) = split_subkind(&det.subkind);
        let wy_event = match WyckoffStructureTracker::event_from_detector_name(&event_name) {
            Some(e) => e,
            None => continue,
        };
        let (bar_idx, price, time_ms) = last_anchor(&det.anchors);
        let anchors_hi = anchor_extrema(&det.anchors, true);
        let anchors_lo = anchor_extrema(&det.anchors, false);

        match active.take() {
            None => {
                // Only a Phase A event may seed a new structure. Anything
                // else is an orphan event belonging to a cycle we
                // haven't caught — skip it rather than write a bogus
                // "starts at D" row.
                if wy_event.phase() != WyckoffPhase::A {
                    active = None;
                    continue;
                }
                let schematic = schematic_for(variant, wy_event);
                let mut tracker = WyckoffStructureTracker::new(schematic, price, price);
                tracker.record_event_with_time(wy_event, bar_idx, price, det.structural_score as f64, time_ms);
                let hi = anchors_hi.max(price);
                let lo = anchors_lo.min(price);
                if hi > lo {
                    tracker.range_top = hi;
                    tracker.range_bottom = lo;
                }
                let events_json = serde_json::to_value(&tracker.events)?;
                let id = if dry_run {
                    Uuid::new_v4()
                } else {
                    insert_wyckoff_structure(
                        pool,
                        &WyckoffStructureInsert {
                            symbol,
                            interval: timeframe,
                            exchange,
                            segment: "futures",
                            schematic: schematic.as_str(),
                            current_phase: tracker.current_phase.as_str(),
                            range_top: tracker.range_top,
                            range_bottom: tracker.range_bottom,
                            creek_level: tracker.creek,
                            ice_level: tracker.ice,
                            events_json,
                            confidence: tracker.confidence(),
                        },
                    ).await?
                };
                active = Some(Active { id, tracker, range_hi: hi, range_lo: lo });
            }
            Some(mut act) => {
                // Structure TTL — matches orchestrator. Wyckoff cycles
                // are localised; if the new event is many bars after
                // the last one, this is a different cycle. Fail the
                // active row and, if the new event is Phase A, reseed.
                let ttl_bars: u64 = match timeframe {
                    "1m" | "3m" | "5m" => 500,
                    "15m" | "30m" | "1h" => 400,
                    "2h" => 350,
                    "4h" => 300,
                    "6h" | "8h" => 250,
                    "12h" => 200,
                    "1d" => 120,
                    "3d" | "1w" => 80,
                    "1M" => 36,
                    _ => 300,
                };
                let last_ev_idx = act.tracker.events.last().map(|e| e.bar_index).unwrap_or(0);
                if bar_idx.saturating_sub(last_ev_idx) > ttl_bars {
                    let reason = format!(
                        "structure TTL exceeded ({} bars since last event; limit {ttl_bars} on {timeframe})",
                        bar_idx.saturating_sub(last_ev_idx)
                    );
                    if !dry_run { fail_wyckoff_structure(pool, act.id, &reason).await?; }
                    failed += 1;
                    // Reseed only if this event is Phase A.
                    if wy_event.phase() == WyckoffPhase::A {
                        let schematic = schematic_for(variant, wy_event);
                        let mut tracker = WyckoffStructureTracker::new(schematic, price, price);
                        tracker.record_event_with_time(wy_event, bar_idx, price, det.structural_score as f64, time_ms);
                        let events_json = serde_json::to_value(&tracker.events)?;
                        let id = if dry_run {
                            Uuid::new_v4()
                        } else {
                            insert_wyckoff_structure(
                                pool,
                                &WyckoffStructureInsert {
                                    symbol,
                                    interval: timeframe,
                                    exchange,
                                    segment: "futures",
                                    schematic: schematic.as_str(),
                                    current_phase: tracker.current_phase.as_str(),
                                    range_top: tracker.range_top,
                                    range_bottom: tracker.range_bottom,
                                    creek_level: tracker.creek,
                                    ice_level: tracker.ice,
                                    events_json,
                                    confidence: tracker.confidence(),
                                },
                            ).await?
                        };
                        active = Some(Active { id, tracker, range_hi: price, range_lo: price });
                    } else {
                        active = None;
                    }
                    continue;
                }

                let prior_schematic = act.tracker.schematic;
                act.tracker.record_event_with_time(
                    wy_event, bar_idx, price, det.structural_score as f64, time_ms,
                );
                // Track rolling range from every anchor seen.
                if anchors_hi > act.range_hi { act.range_hi = anchors_hi; }
                if anchors_lo < act.range_lo { act.range_lo = anchors_lo; }
                if act.range_hi > act.range_lo {
                    act.tracker.range_top = act.range_hi;
                    act.tracker.range_bottom = act.range_lo;
                }

                let was_bull = matches!(
                    prior_schematic,
                    WyckoffSchematic::Accumulation | WyckoffSchematic::ReAccumulation,
                );
                let now_bull = matches!(
                    act.tracker.schematic,
                    WyckoffSchematic::Accumulation | WyckoffSchematic::ReAccumulation,
                );
                // Mirror the orchestrator's schematic-lock delay: only
                // call a family flip a failure once the structure has
                // actually reached Phase C (Spring/UTAD territory). Keep
                // replay output aligned with production so reports match.
                let family_flipped = was_bull != now_bull
                    && act.tracker.current_phase >= WyckoffPhase::C;
                let events_json = serde_json::to_value(&act.tracker.events)?;

                if !dry_run {
                    update_wyckoff_structure(
                        pool, act.id,
                        act.tracker.current_phase.as_str(),
                        act.tracker.schematic.as_str(),
                        act.tracker.range_top, act.tracker.range_bottom,
                        act.tracker.creek, act.tracker.ice,
                        &events_json, act.tracker.confidence(),
                    ).await?;
                }

                if act.tracker.current_phase == WyckoffPhase::E {
                    if !dry_run { complete_wyckoff_structure(pool, act.id).await?; }
                    completed += 1;
                    active = None;
                } else if family_flipped {
                    let reason = format!(
                        "schematic flipped {} → {} via {}",
                        prior_schematic.as_str(),
                        act.tracker.schematic.as_str(),
                        wy_event.as_str(),
                    );
                    if !dry_run { fail_wyckoff_structure(pool, act.id, &reason).await?; }
                    failed += 1;
                    // If the flipping event is itself a Phase A seed,
                    // start a fresh structure from it; otherwise drop.
                    if wy_event.phase() == WyckoffPhase::A {
                        let schematic = schematic_for(variant, wy_event);
                        let mut tracker = WyckoffStructureTracker::new(schematic, price, price);
                        tracker.record_event_with_time(wy_event, bar_idx, price, det.structural_score as f64, time_ms);
                        let events_json = serde_json::to_value(&tracker.events)?;
                        let id = if dry_run {
                            Uuid::new_v4()
                        } else {
                            insert_wyckoff_structure(
                                pool,
                                &WyckoffStructureInsert {
                                    symbol,
                                    interval: timeframe,
                                    exchange,
                                    segment: "futures",
                                    schematic: schematic.as_str(),
                                    current_phase: tracker.current_phase.as_str(),
                                    range_top: tracker.range_top,
                                    range_bottom: tracker.range_bottom,
                                    creek_level: tracker.creek,
                                    ice_level: tracker.ice,
                                    events_json,
                                    confidence: tracker.confidence(),
                                },
                            ).await?
                        };
                        active = Some(Active { id, tracker, range_hi: price, range_lo: price });
                    }
                } else {
                    active = Some(act);
                }
            }
        }
    }

    // Tail: if active and in Phase D, try synthetic Markup/Markdown
    // breakout injection (mirrors orchestrator P9). Needs recent bars.
    let active_count = if let Some(mut act) = active {
        if act.tracker.current_phase == WyckoffPhase::D {
            if let Ok(mut recent) = list_recent_bars(pool, exchange, "futures", symbol, timeframe, 60).await {
                recent.reverse();
                try_synth_phase_e(&mut act.tracker, &recent);
                if act.tracker.current_phase == WyckoffPhase::E {
                    let events_json = serde_json::to_value(&act.tracker.events)?;
                    if !dry_run {
                        update_wyckoff_structure(
                            pool, act.id,
                            act.tracker.current_phase.as_str(),
                            act.tracker.schematic.as_str(),
                            act.tracker.range_top, act.tracker.range_bottom,
                            act.tracker.creek, act.tracker.ice,
                            &events_json, act.tracker.confidence(),
                        ).await?;
                        complete_wyckoff_structure(pool, act.id).await?;
                    }
                    completed += 1;
                    0
                } else {
                    1
                }
            } else { 1 }
        } else { 1 }
    } else { 0 };

    println!(
        "  {CYAN}{symbol:<10}{RESET} {timeframe:<4}  dets={:<5}  completed={completed}  failed={failed}  active={active_count}",
        dets.len(),
    );

    Ok((completed, failed, active_count))
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

/// Split `"selling_climax_accumulation"` → (`"selling_climax"`, `"accumulation"`).
fn split_subkind(subkind: &str) -> (String, &str) {
    // Last token is the schematic variant; everything before is the event.
    match subkind.rsplit_once('_') {
        Some((ev, variant)) if matches!(variant, "accumulation" | "distribution" | "reaccumulation" | "redistribution" | "bull" | "bear") => {
            (ev.to_string(), variant)
        }
        _ => (subkind.to_string(), ""),
    }
}

fn schematic_for(variant: &str, ev: WyckoffEvent) -> WyckoffSchematic {
    match variant {
        "accumulation" => WyckoffSchematic::Accumulation,
        "distribution" => WyckoffSchematic::Distribution,
        "reaccumulation" => WyckoffSchematic::ReAccumulation,
        "redistribution" => WyckoffSchematic::ReDistribution,
        _ => {
            if matches!(
                ev,
                WyckoffEvent::SC | WyckoffEvent::Spring | WyckoffEvent::SOS
                | WyckoffEvent::LPS | WyckoffEvent::JAC
            ) {
                WyckoffSchematic::Accumulation
            } else {
                WyckoffSchematic::Distribution
            }
        }
    }
}

fn last_anchor(anchors: &serde_json::Value) -> (u64, f64, Option<i64>) {
    let arr = anchors.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let Some(a) = arr.last() else { return (0, 0.0, None); };
    let bar_index = a.get("bar_index").and_then(|v| v.as_u64()).unwrap_or(0);
    let price = a
        .get("price")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| a.get("price").and_then(|v| v.as_f64()))
        .unwrap_or(0.0);
    let time_ms = a.get("time").and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc).timestamp_millis());
    (bar_index, price, time_ms)
}

fn anchor_extrema(anchors: &serde_json::Value, max: bool) -> f64 {
    let arr = match anchors.as_array() { Some(a) => a, None => return if max { f64::MIN } else { f64::MAX } };
    let mut acc = if max { f64::MIN } else { f64::MAX };
    for a in arr {
        let p = a.get("price").and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| a.get("price").and_then(|v| v.as_f64()))
            .unwrap_or(0.0);
        if p == 0.0 { continue; }
        if max && p > acc { acc = p; }
        if !max && p < acc { acc = p; }
    }
    acc
}

/// Mirror of `maybe_inject_markup_markdown` in v2_detection_orchestrator.rs.
fn try_synth_phase_e(
    tracker: &mut WyckoffStructureTracker,
    recent: &[qtss_storage::MarketBarRow],
) {
    use rust_decimal::prelude::ToPrimitive;
    if tracker.events.iter().any(|e| matches!(e.event, WyckoffEvent::Markup | WyckoffEvent::Markdown)) {
        return;
    }
    let bullish = matches!(
        tracker.schematic,
        WyckoffSchematic::Accumulation | WyckoffSchematic::ReAccumulation,
    );
    let (top, bot) = (tracker.range_top, tracker.range_bottom);
    if top <= 0.0 || bot <= 0.0 || top <= bot { return; }
    let threshold = if bullish { top * 1.005 } else { bot * 0.995 };
    if recent.len() < 10 { return; }
    let window = &recent[recent.len().saturating_sub(30)..];
    let confirmed = window.iter().filter(|r| {
        let c = r.close.to_f64().unwrap_or(0.0);
        if bullish { c > threshold } else { c < threshold }
    }).count();
    if confirmed * 10 < window.len() * 6 { return; }
    let last = match window.last() { Some(r) => r, None => return };
    let price = last.close.to_f64().unwrap_or(0.0);
    let ev = if bullish { WyckoffEvent::Markup } else { WyckoffEvent::Markdown };
    let time_ms = Some(last.open_time.timestamp_millis());
    let bar_idx = tracker.events.last().map(|e| e.bar_index + 1).unwrap_or(0);
    tracker.record_event_with_time(ev, bar_idx, price, 0.55, time_ms);
    let _ = json!({});
}
