// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! Wyckoff writer — 12th engine-dispatch member (Faz 14).
//!
//! Reads bars, runs `qtss_wyckoff::detect_events`, upserts each event
//! into `detections` with `pattern_family = 'wyckoff'` and a
//! `<event>_<variant>` subkind. Phase tracker output (A/B/C/D/E)
//! written to `raw_meta.phase` so the chart overlay can colour the
//! range.

use async_trait::async_trait;
use chrono::Utc;
use qtss_domain::v2::bar::Bar as DomainBar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::timeframe::Timeframe;
use qtss_storage::market_bars::{self, MarketBarRow};
use qtss_wyckoff::{
    boost_with_phase_c_events, detect_cycles_for_slot,
    detect_cycles_from_elliott, detect_events, detect_ranges,
    dedupe_consecutive_same_phase, enforce_non_overlap,
    filter_phase_c_events_in_context, merge_cycles_with_confluence,
    ElliottSegment, ElliottSegmentKind, WyckoffBias, WyckoffConfig,
    WyckoffCycle, WyckoffCyclePhase, WyckoffCycleSource, WyckoffEvent,
    WyckoffPhaseTracker, WyckoffRange,
};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct WyckoffWriter;

#[async_trait]
impl WriterTask for WyckoffWriter {
    fn family_name(&self) -> &'static str {
        "wyckoff"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let syms = symbols::list_enabled(pool).await?;
        let cfg = load_config(pool).await;
        for sym in &syms {
            match process_symbol(pool, sym, &cfg).await {
                Ok(n) => {
                    stats.series_processed += 1;
                    stats.rows_upserted += n;
                }
                Err(e) => warn!(
                    exchange = %sym.exchange,
                    symbol = %sym.symbol,
                    tf = %sym.interval,
                    %e,
                    "wyckoff: series failed"
                ),
            }
        }
        Ok(stats)
    }
}

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    cfg: &WyckoffConfig,
) -> anyhow::Result<usize> {
    let raw = market_bars::list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        2000,
    )
    .await?;
    if raw.len() < 60 {
        return Ok(0);
    }
    let chrono: Vec<MarketBarRow> = raw.into_iter().rev().collect();
    let instrument = build_instrument(sym);
    let tf = parse_tf(&sym.interval);
    let bars: Vec<DomainBar> = chrono
        .iter()
        .map(|r| to_domain_bar(r, &instrument, tf))
        .collect();

    let raw_events = detect_events(&bars, cfg);

    // FAZ 25.4.E — Phase-C context filter. Run range detection FIRST
    // so we know which Accumulation / Distribution windows are active,
    // then drop Spring/UTAD events that fall outside their canonical
    // schematic context (Spring inside Distribution OR no range at
    // all → suppress; same for UTAD inside Accumulation). Per Wyckoff
    // doctrine each range's Phase C contains EXACTLY ONE Spring (or
    // UTAD), so the filter also collapses multiple within-range
    // candidates to the strongest score.
    //
    // Why filter at the writer level rather than inside detect_events?
    // Range detection (detect_ranges) consumes the event stream — if
    // we filtered before range detection, the range boundaries would
    // be miscomputed (Spring contributes to range_LOW). So order is:
    //   1. detect_events → all candidates
    //   2. detect_ranges(events) → range windows
    //   3. filter_phase_c_events_in_context → drop spam Spring/UTAD
    //   4. persist filtered events + the same ranges
    let mut sorted = raw_events.clone();
    sorted.sort_by_key(|e| e.bar_index);
    let raw_ranges = detect_ranges(&sorted);
    let events = filter_phase_c_events_in_context(raw_events, &raw_ranges);

    // Feed the phase tracker on the FILTERED event stream so phase
    // labels reflect the canonical Wyckoff schematic rather than
    // every spam wick.
    let mut tracker = WyckoffPhaseTracker::new();
    let mut sorted = events.clone();
    sorted.sort_by_key(|e| e.bar_index);
    for e in &sorted {
        tracker.feed(e);
    }
    let phase_str = format!("{:?}", tracker.phase()).to_lowercase();
    let bias_str = format!("{:?}", tracker.bias()).to_lowercase();

    let mut written = 0usize;
    for ev in &events {
        written += write_event(pool, sym, &chrono, ev, &phase_str, &bias_str).await?;
    }

    // FAZ 25.4.B — schematic range boxes. The user asked for the
    // Accumulation / Distribution rectangle that frames events,
    // matching the TradingView Wyckoff annotation convention. We
    // group sorted events through detect_ranges() and persist each
    // as its own detection row with pattern_family='wyckoff' +
    // subkind='range_accumulation' / 'range_distribution'. The
    // chart renders these as a primitive rectangle behind the event
    // markers.
    //
    // Sweep stale ranges + cycles before writing fresh ones so old
    // boxes/segments don't pile up at the same slot.
    let _ = sqlx::query(
        r#"DELETE FROM detections
            WHERE exchange = $1 AND segment = $2
              AND symbol = $3 AND timeframe = $4
              AND pattern_family = 'wyckoff'
              AND mode = 'live'
              AND (subkind LIKE 'range_%' OR subkind LIKE 'cycle_%')"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .execute(pool)
    .await;

    // Schematic accumulation / distribution range boxes (slot 0 only —
    // they're a per-symbol annotation, not a per-degree breakdown).
    // Recompute against the FILTERED sorted stream so range_low /
    // range_high reflect the canonical (per-range strongest) Spring
    // / UTAD instead of the raw spam.
    let ranges = detect_ranges(&sorted);
    for r in &ranges {
        written += write_range(pool, sym, &chrono, r).await?;
    }

    // FAZ 25.4.D + E — hybrid four-phase macro cycle per Z-degree slot.
    //
    // User asked: "Accumulation, Markup, Distribution, Markdown
    // fazları elliot dalgalarla ilişkilendirme en doğru yöntem değil
    // mi" + "z1-z5 herbiri için kutu". Answer (per Pruden's canonical
    // mapping in docs/ELLIOTT_WYCKOFF_INTEGRATION.md §VII): yes, but
    // Elliott alone has wave-count subjectivity. The robust answer is
    // a HYBRID — combine three sources per slot:
    //
    //   1. Event-driven (SC/BC/Bu/Sos/Sow events) — `source=Event`
    //   2. Elliott-anchored (motive + abc rows) — `source=Elliott`
    //   3. Confluence (overlap of 1+2 on same phase) — `source=Confluent`
    //
    // The chart renderer differentiates via raw_meta.source; downstream
    // composite scoring boosts confidence on confluent tiles.
    let tape_end_bar = chrono.len().saturating_sub(1);
    let tape_end_price = chrono
        .last()
        .and_then(|r| r.close.to_string().parse::<f64>().ok())
        .unwrap_or(0.0);
    for slot in 0i16..=5 {
        // (1) Event-driven, slot-aware.
        let min_score = slot_min_score(slot);
        let event_cycles = detect_cycles_for_slot(
            &sorted,
            &bars,
            min_score,
            tape_end_bar,
            tape_end_price,
        );

        // (2) Elliott-anchored — query motive + abc rows for this slot.
        let segments = load_elliott_segments(pool, sym, slot, &chrono).await?;
        let elliott_cycles = detect_cycles_from_elliott(
            &segments,
            &bars,
            tape_end_bar,
            tape_end_price,
        );

        // (3a) Phase-C event boost: Spring (Wyckoff Accumulation Phase
        // C) at the start of an Elliott Markup tile, OR UTAD
        // (Distribution Phase C) at the start of a Markdown tile,
        // upgrades the tile's source to Confluent. This captures
        // Pruden's canonical highest-conviction signal (Spring = W2 dip
        // = Markup ignition; UTAD = B-wave = Markdown trigger).
        let boost_window_bars: usize = 5;
        let elliott_cycles =
            boost_with_phase_c_events(elliott_cycles, &sorted, boost_window_bars);

        // (3b) Hybrid confluence merge (50% overlap on same phase).
        let merged = merge_cycles_with_confluence(
            event_cycles,
            elliott_cycles,
            &bars,
            0.5,
        );

        // (3c) BUG3 round 1 — collapse ADJACENT same-phase tiles
        // (e.g. 3 sequential Accumulations) into one spanning tile.
        let merged = dedupe_consecutive_same_phase(merged);

        // (3d) BUG3 round 2 (2026-04-27) — resolve OVERLAPPING
        // DIFFERENT-phase tiles. Event-driven and Elliott-anchored
        // detectors run in parallel; when they disagree on phase
        // boundaries (e.g. Event flags Distribution at the BC
        // climax while Elliott still calls the same window Markup
        // because W5 has not printed yet), the merger leaves both
        // tiles in place and the chart renders them stacked. The
        // user reported this on ETHUSDT 1h Z1: a giant Distribution
        // box with Markdown and Accumulation tiles drawn INSIDE.
        // `enforce_non_overlap` clips lower-priority tiles to the
        // gaps where the higher-priority detector had no opinion,
        // producing a single non-overlapping timeline per slot.
        let merged = enforce_non_overlap(merged);

        // (3e) Re-fuse — clipping in (3d) can create new adjacent
        // same-phase tiles when a higher-priority middle tile is
        // skipped past. Run dedupe a second time to collapse them.
        let merged = dedupe_consecutive_same_phase(merged);

        // (3f) BUG3 round 2 — clean stale cycle_* rows for this slot
        // before writing the fresh non-overlapping set. write_cycle's
        // ON CONFLICT key includes (start_time, end_time), which means
        // old tiles whose boundaries shifted under enforce_non_overlap
        // get LEFT BEHIND and the chart still renders them stacked
        // alongside the corrected ones. Delete first, then insert.
        // The cycle tile count per slot is small (~tens) so this is
        // cheap.
        sqlx::query(
            r#"DELETE FROM detections
                WHERE exchange = $1 AND segment = $2
                  AND symbol = $3 AND timeframe = $4
                  AND slot = $5
                  AND pattern_family = 'wyckoff'
                  AND subkind LIKE 'cycle\_%' ESCAPE '\'
                  AND mode = 'live'"#,
        )
        .bind(&sym.exchange)
        .bind(&sym.segment)
        .bind(&sym.symbol)
        .bind(&sym.interval)
        .bind(slot)
        .execute(pool)
        .await?;

        for c in &merged {
            written += write_cycle(pool, sym, &chrono, c, slot).await?;
        }
    }
    Ok(written)
}

/// Pull Elliott motive + abc detections for a given slot, convert
/// them into the `ElliottSegment` shape the cycle detector expects.
/// `chrono` provides bar-index → time mapping (anchors store both;
/// we trust bar_index from the row directly).
async fn load_elliott_segments(
    pool: &PgPool,
    sym: &EngineSymbol,
    slot: i16,
    chrono: &[MarketBarRow],
) -> anyhow::Result<Vec<ElliottSegment>> {
    let rows = sqlx::query(
        r#"SELECT id::text AS id, pattern_family, direction,
                  start_bar, end_bar, anchors
             FROM detections
            WHERE exchange = $1 AND segment = $2
              AND symbol = $3 AND timeframe = $4
              AND slot = $5
              AND mode = 'live'
              AND pattern_family IN ('motive', 'abc')
              AND invalidated = false
            ORDER BY start_bar"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(slot)
    .fetch_all(pool)
    .await?;

    let bar_count = chrono.len();
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let id: String = r.try_get("id").unwrap_or_default();
        let family: String = r.try_get("pattern_family").unwrap_or_default();
        let direction: i16 = r.try_get("direction").unwrap_or(0);
        let start_bar: i64 = r.try_get("start_bar").unwrap_or(0);
        let end_bar: i64 = r.try_get("end_bar").unwrap_or(0);
        let anchors: Value = r.try_get("anchors").unwrap_or(Value::Null);

        let kind = match family.as_str() {
            "motive" => ElliottSegmentKind::Motive,
            "abc" => ElliottSegmentKind::Abc,
            _ => continue,
        };
        let bullish = direction >= 0;
        let s_bar = (start_bar.max(0) as usize).min(bar_count.saturating_sub(1));
        let e_bar = (end_bar.max(0) as usize).min(bar_count.saturating_sub(1));
        // Parse the full anchors array into `wave_anchors` —
        // motive: 6 entries (W0..W5); abc: 4 entries (X0, A, B, C).
        // Each anchor JSON object has `bar_index` + `price`. Out-of-
        // range bar indices clip to the chrono slice.
        let mut wave_anchors: Vec<(usize, f64)> = Vec::new();
        if let Some(arr) = anchors.as_array() {
            for v in arr {
                let bi = v.get("bar_index").and_then(|x| x.as_i64()).unwrap_or(-1);
                let pr = v.get("price").and_then(|x| x.as_f64()).unwrap_or(0.0);
                if bi < 0 || !pr.is_finite() {
                    continue;
                }
                let bar = (bi as usize).min(bar_count.saturating_sub(1));
                wave_anchors.push((bar, pr));
            }
        }
        let start_price = wave_anchors
            .first()
            .map(|(_, p)| *p)
            .unwrap_or(0.0);
        let end_price = wave_anchors
            .last()
            .map(|(_, p)| *p)
            .unwrap_or(0.0);

        out.push(ElliottSegment {
            kind,
            bullish,
            start_bar: s_bar,
            end_bar: e_bar,
            start_price,
            end_price,
            source_id: Some(id),
            wave_anchors,
        });
    }
    Ok(out)
}

/// Slot → minimum event score for a cycle to count at that degree.
/// Slot 0 mirrors `WyckoffConfig::min_structural_score` (base 0.55);
/// each higher slot tightens by 0.05, so slot 5 = 0.80 (only the very
/// strongest climaxes survive).
fn slot_min_score(slot: i16) -> f32 {
    let base: f32 = 0.55;
    let step: f32 = 0.05;
    base + (slot.max(0) as f32) * step
}

async fn write_cycle(
    pool: &PgPool,
    sym: &EngineSymbol,
    chrono: &[MarketBarRow],
    c: &WyckoffCycle,
    slot: i16,
) -> anyhow::Result<usize> {
    let phase_str = match c.phase {
        WyckoffCyclePhase::Accumulation => "accumulation",
        WyckoffCyclePhase::Markup => "markup",
        WyckoffCyclePhase::Distribution => "distribution",
        WyckoffCyclePhase::Markdown => "markdown",
    };
    let subkind = format!("cycle_{phase_str}");
    let direction: i16 = match c.phase {
        WyckoffCyclePhase::Accumulation | WyckoffCyclePhase::Markup => 1,
        WyckoffCyclePhase::Distribution | WyckoffCyclePhase::Markdown => -1,
    };
    let start_time = chrono
        .get(c.start_bar)
        .map(|b| b.open_time)
        .unwrap_or_else(Utc::now);
    let end_time = chrono
        .get(c.end_bar)
        .map(|b| b.open_time)
        .unwrap_or_else(Utc::now);
    // anchors = corners of the cycle band the chart renderer needs:
    //   anchors[0] = start at start_price
    //   anchors[1] = end at end_price
    let anchors = json!([
        {
            "label_override": phase_str,
            "bar_index": c.start_bar as i64,
            "price": c.start_price,
            "time": start_time,
        },
        {
            "label_override": phase_str,
            "bar_index": c.end_bar as i64,
            "price": c.end_price,
            "time": end_time,
        }
    ]);
    let source_str = match c.source {
        WyckoffCycleSource::Event => "event",
        WyckoffCycleSource::Elliott => "elliott",
        WyckoffCycleSource::Confluent => "confluent",
    };
    let raw_meta = json!({
        "phase":              phase_str,
        "source":             source_str,
        "source_pattern_id":  c.source_pattern_id,
        "start_price":        c.start_price,
        "end_price":          c.end_price,
        "phase_high":         c.phase_high,
        "phase_low":          c.phase_low,
        "completed":          c.completed,
        "slot":               slot,
        "kind":               "wyckoff_cycle",
    });
    sqlx::query(
        r#"INSERT INTO detections
              (exchange, segment, symbol, timeframe, slot,
               pattern_family, subkind, direction,
               start_bar, end_bar, start_time, end_time,
               anchors, invalidated, raw_meta, mode)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,'live')
           ON CONFLICT (exchange, segment, symbol, timeframe, slot,
                        pattern_family, subkind, start_time, end_time, mode)
           DO UPDATE SET
               direction  = EXCLUDED.direction,
               anchors    = EXCLUDED.anchors,
               raw_meta   = EXCLUDED.raw_meta,
               updated_at = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(slot)
    .bind("wyckoff")
    .bind(&subkind)
    .bind(direction)
    .bind(c.start_bar as i64)
    .bind(c.end_bar as i64)
    .bind(start_time)
    .bind(end_time)
    .bind(&anchors)
    // BUG FIX: previously bound `c.completed` here. The `invalidated`
    // column means "no longer valid / should not render" — but
    // `completed` means "this phase has rotated to the next" (a
    // historical / closed tile), which is precisely what the chart
    // SHOULD render. Mixing them caused every closed Markup/
    // Accumulation/Markdown tile to vanish from the API response
    // (filter `invalidated = false`). Cycle tiles never invalidate;
    // they only complete. Track completion via raw_meta.completed.
    .bind(false)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

async fn write_range(
    pool: &PgPool,
    sym: &EngineSymbol,
    chrono: &[MarketBarRow],
    r: &WyckoffRange,
) -> anyhow::Result<usize> {
    let bias_str = match r.bias {
        WyckoffBias::Accumulation => "accumulation",
        WyckoffBias::Distribution => "distribution",
        WyckoffBias::Neutral => return Ok(0),
    };
    let subkind = format!("range_{bias_str}");
    let direction: i16 = match r.bias {
        WyckoffBias::Accumulation => 1,
        WyckoffBias::Distribution => -1,
        _ => 0,
    };
    let start_time = chrono
        .get(r.start_bar)
        .map(|c| c.open_time)
        .unwrap_or_else(Utc::now);
    let end_time = chrono
        .get(r.end_bar)
        .map(|c| c.open_time)
        .unwrap_or_else(Utc::now);
    // anchors = corners of the rectangle so the chart renderer can
    // draw a filled box without an extra fetch:
    //   anchors[0] = start, low corner
    //   anchors[1] = end, high corner
    let anchors = json!([
        {
            "label_override": bias_str,
            "bar_index": r.start_bar as i64,
            "price": r.range_low,
            "time": start_time,
        },
        {
            "label_override": bias_str,
            "bar_index": r.end_bar as i64,
            "price": r.range_high,
            "time": end_time,
        }
    ]);
    let phase_str = format!("{:?}", r.phase).to_lowercase();
    let raw_meta = json!({
        "phase":          phase_str,
        "bias":           bias_str,
        "range_high":     r.range_high,
        "range_low":      r.range_low,
        "event_count":    r.event_indices.len(),
        "completed":      r.completed,
        "kind":           "wyckoff_range",
    });
    sqlx::query(
        r#"INSERT INTO detections
              (exchange, segment, symbol, timeframe, slot,
               pattern_family, subkind, direction,
               start_bar, end_bar, start_time, end_time,
               anchors, invalidated, raw_meta, mode)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,'live')
           ON CONFLICT (exchange, segment, symbol, timeframe, slot,
                        pattern_family, subkind, start_time, end_time, mode)
           DO UPDATE SET
               direction  = EXCLUDED.direction,
               anchors    = EXCLUDED.anchors,
               raw_meta   = EXCLUDED.raw_meta,
               updated_at = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(0i16)
    .bind("wyckoff")
    .bind(&subkind)
    .bind(direction)
    .bind(r.start_bar as i64)
    .bind(r.end_bar as i64)
    .bind(start_time)
    .bind(end_time)
    .bind(&anchors)
    // Same fix as write_cycle — completion is NOT invalidation.
    // `invalidated` is for "should not render" semantics; ranges
    // never invalidate, they just close. Track completion via
    // raw_meta.completed.
    .bind(false)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

fn build_instrument(sym: &EngineSymbol) -> Instrument {
    let venue = match sym.exchange.as_str() {
        "binance" => Venue::Binance,
        "bybit" => Venue::Bybit,
        "okx" => Venue::Okx,
        other => Venue::Custom(other.to_string()),
    };
    let asset_class = match sym.segment.as_str() {
        "spot" => AssetClass::CryptoSpot,
        "futures" => AssetClass::CryptoFutures,
        _ => AssetClass::CryptoFutures,
    };
    Instrument {
        venue,
        asset_class,
        symbol: sym.symbol.clone(),
        quote_ccy: "USDT".to_string(),
        tick_size: Decimal::ZERO,
        lot_size: Decimal::ZERO,
        session: SessionCalendar::binance_24x7(),
    }
}

fn to_domain_bar(r: &MarketBarRow, inst: &Instrument, tf: Timeframe) -> DomainBar {
    DomainBar {
        instrument: inst.clone(),
        timeframe: tf,
        open_time: r.open_time,
        open: r.open,
        high: r.high,
        low: r.low,
        close: r.close,
        volume: r.volume,
        closed: true,
    }
}

fn parse_tf(s: &str) -> Timeframe {
    s.parse::<Timeframe>().unwrap_or(Timeframe::H1)
}

async fn write_event(
    pool: &PgPool,
    sym: &EngineSymbol,
    chrono: &[MarketBarRow],
    ev: &WyckoffEvent,
    phase: &str,
    bias: &str,
) -> anyhow::Result<usize> {
    let subkind = format!("{}_{}", ev.kind.as_str(), ev.variant);
    let direction: i16 = match ev.variant {
        "bull" => 1,
        "bear" => -1,
        _ => 0,
    };
    let bar_time = chrono
        .get(ev.bar_index)
        .map(|r| r.open_time)
        .unwrap_or_else(Utc::now);
    let anchors = json!([
        {
            "label_override": ev.kind.as_str().to_uppercase(),
            "bar_index": ev.bar_index as i64,
            "price": ev.reference_price,
            "time": bar_time,
        }
    ]);
    let raw_meta = json!({
        "score":           ev.score,
        "volume_ratio":    ev.volume_ratio,
        "range_ratio":     ev.range_ratio,
        "reference_price": ev.reference_price,
        "note":            ev.note,
        "event_kind":      ev.kind.as_str(),
        "phase":           phase,
        "bias":            bias,
    });
    sqlx::query(
        r#"INSERT INTO detections
              (exchange, segment, symbol, timeframe, slot,
               pattern_family, subkind, direction,
               start_bar, end_bar, start_time, end_time,
               anchors, invalidated, raw_meta, mode)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,'live')
           ON CONFLICT (exchange, segment, symbol, timeframe, slot,
                        pattern_family, subkind, start_time, end_time, mode)
           DO UPDATE SET
               direction  = EXCLUDED.direction,
               anchors    = EXCLUDED.anchors,
               raw_meta   = EXCLUDED.raw_meta,
               updated_at = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(0i16)
    .bind("wyckoff")
    .bind(&subkind)
    .bind(direction)
    .bind(ev.bar_index as i64)
    .bind(ev.bar_index as i64)
    .bind(bar_time)
    .bind(bar_time)
    .bind(&anchors)
    .bind(false)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

async fn load_config(pool: &PgPool) -> WyckoffConfig {
    let mut cfg = WyckoffConfig::default();
    let rows = sqlx::query(
        r#"SELECT config_key, value FROM system_config
            WHERE module = 'wyckoff' AND config_key LIKE 'thresholds.%'"#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for r in rows {
        let key: String = r.try_get("config_key").unwrap_or_default();
        let val: Value = r.try_get("value").unwrap_or(Value::Null);
        let Some(v) = val.get("value").and_then(|v| v.as_f64()) else { continue };
        let k = key.trim_start_matches("thresholds.");
        match k {
            "climax_volume_mult" => cfg.climax_volume_mult = v,
            "climax_range_atr_mult" => cfg.climax_range_atr_mult = v,
            "spring_wick_max_pct" => cfg.spring_wick_max_pct = v,
            "sos_amplifier" => cfg.sos_amplifier = v,
            // FAZ 26 backlog (B-CTX-MM-1) — Wyckoff volume gate.
            "spring_min_volume_mult" => cfg.spring_min_volume_mult = v,
            "spring_max_volume_mult" => cfg.spring_max_volume_mult = v,
            _ => {}
        }
    }
    cfg
}
