//! `engine_symbols` → `market_bars` coverage: REST history + health/gap metrics.
//!
//! State machine per series (tracked in `backfill_progress`):
//!   pending → backfilling → verifying → complete → live
//!
//! - **pending/backfilling**: fetch history from listing to now, resume-safe
//! - **verifying**: count bars + scan gaps, decide complete or retry
//! - **complete**: all history present, only live updates needed
//! - **live**: complete + real-time feed active

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use qtss_domain::bar::MarketBarProvider;
use tokio::sync::Semaphore;
use qtss_storage::{
    count_market_bars_series, get_or_create_backfill_progress, list_enabled_engine_symbols,
    list_recent_bar_open_times_desc, mark_backfill_finished, mark_backfill_started, mark_live,
    record_backfill_error, resolve_system_u64, resolve_worker_tick_secs,
    update_backfill_cursor, update_verification, upsert_engine_symbol_ingestion_state,
    BackfillProgressRow, EngineSymbolRow,
};
use sqlx::PgPool;
use tracing::{debug, info, warn};

fn binance_kline_interval_seconds(iv: &str) -> Option<i64> {
    match iv.trim() {
        "1m" => Some(60),
        "3m" => Some(180),
        "5m" => Some(300),
        "15m" => Some(900),
        "30m" => Some(1800),
        "1h" | "60m" => Some(3600),
        "2h" => Some(7200),
        "4h" => Some(14400),
        "6h" => Some(21600),
        "8h" => Some(28800),
        "12h" => Some(43200),
        "1d" | "1D" => Some(86_400),
        "3d" | "3D" => Some(259_200),
        "1w" | "1W" => Some(604_800),
        _ => None,
    }
}

fn segment_db(segment: &str) -> &'static str {
    match segment.trim().to_lowercase().as_str() {
        "future" | "futures" | "usdt_futures" | "fapi" => "futures",
        _ => "spot",
    }
}

/// Count gaps in ascending time order.
fn scan_gaps_asc(times_asc: &[DateTime<Utc>], expected_sec: i64) -> (i32, Option<i32>) {
    if times_asc.len() < 2 || expected_sec <= 0 {
        return (0, None);
    }
    let tol = (expected_sec as f64 * 0.15).max(2.0);
    let mut gaps = 0_i32;
    let mut max_excess: i64 = 0;
    for w in times_asc.windows(2) {
        let dt = w[1].signed_duration_since(w[0]).num_seconds();
        if dt <= 0 {
            gaps += 1;
            continue;
        }
        let diff = (dt - expected_sec).abs();
        if diff as f64 > tol {
            gaps += 1;
            max_excess = max_excess.max(dt - expected_sec);
        }
    }
    let max_gap_i32 = if max_excess > 0 {
        Some(max_excess.clamp(0, i32::MAX as i64) as i32)
    } else {
        None
    };
    (gaps, max_gap_i32)
}

// ─── State Machine: one tick per engine_symbol ──────────────────────

async fn run_one_target(
    pool: &PgPool,
    row: &EngineSymbolRow,
    min_bars: i64,
    gap_window: i64,
    full_history: bool,
    provider: &dyn MarketBarProvider,
) {
    let ex = row.exchange.trim();
    let seg = row.segment.trim();
    let sym = row.symbol.trim();
    let iv = row.interval.trim();

    if !ex.eq_ignore_ascii_case(provider.exchange_id()) {
        return;
    }
    if !provider.is_tradable(sym, seg).await {
        debug!(engine_symbol_id = %row.id, symbol = %sym, "skip — not tradable");
        return;
    }

    // ── Get/create backfill progress ────────────────────────────────
    let progress = match get_or_create_backfill_progress(pool, row.id).await {
        Ok(p) => p,
        Err(e) => {
            warn!(%e, engine_symbol_id = %row.id, "get_or_create_backfill_progress");
            return;
        }
    };

    let exp = binance_kline_interval_seconds(iv);

    match progress.state.as_str() {
        "pending" | "backfilling" => {
            // ── PHASE 1: Historical backfill (resumable) ────────────
            if !full_history && progress.state == "pending" {
                // full_history disabled — skip to cold-start / live mode
                run_cold_start_and_live(pool, row, min_bars, gap_window, provider).await;
                return;
            }
            run_resumable_backfill(pool, row, &progress, provider, exp).await;
        }
        "verifying" => {
            // ── PHASE 2: Verify completeness ────────────────────────
            run_verification(pool, row, gap_window, exp).await;
        }
        "complete" | "live" => {
            // ── PHASE 3: Live updates (stale/gap patch) ─────────────
            run_live_updates(pool, row, min_bars, gap_window, provider, exp).await;
        }
        other => {
            warn!(engine_symbol_id = %row.id, state = %other, "unknown backfill_progress state");
        }
    }
}

/// Phase 1: Resumable full-history backfill — fetches in small blocks (5 pages
/// each) and updates the DB cursor after every block. If the worker crashes,
/// at most one block (~5 000 bars) is lost.
async fn run_resumable_backfill(
    pool: &PgPool,
    row: &EngineSymbolRow,
    progress: &BackfillProgressRow,
    provider: &dyn MarketBarProvider,
    _exp: Option<i64>,
) {
    let sym = row.symbol.trim();
    let iv = row.interval.trim();
    let seg = row.segment.trim();

    let mut cursor_ms: Option<u64> = progress.oldest_fetched.map(|t| {
        (t.timestamp_millis() as u64).saturating_sub(1)
    });
    let mut total_pages = progress.pages_fetched;
    let mut total_bars = progress.bars_upserted;

    info!(
        engine_symbol_id = %row.id,
        symbol = %sym,
        interval = %iv,
        resume_from = ?progress.oldest_fetched,
        pages_so_far = total_pages,
        bars_so_far = total_bars,
        "backfill: resuming history fetch"
    );

    // Mark as backfilling
    let _ = mark_backfill_started(pool, row.id).await;

    // ── Block-by-block loop ────────────────────────────────────────
    // Each block = 5 pages × 1000 bars = 5 000 bars.
    // After each block the cursor is persisted so restarts are cheap.
    // We do up to 20 blocks per tick (≈100 000 bars) then yield.
    const BLOCK_SIZE: i64 = 5_000;
    const MAX_BLOCKS: u32 = 20;

    for block_idx in 0..MAX_BLOCKS {
        let res = match provider
            .backfill_bars_resumable(sym, iv, seg, BLOCK_SIZE, cursor_ms)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(%e, engine_symbol_id = %row.id, symbol = %sym, block = block_idx, "backfill block error");
                let _ = record_backfill_error(pool, row.id, &e.to_string()).await;
                return;
            }
        };

        total_pages += res.pages as i32;
        total_bars += res.upserted;

        // Persist cursor after every block
        let oldest_dt = res.oldest_ms.and_then(|ms| Utc.timestamp_millis_opt(ms as i64).single());
        let newest_dt = res.newest_ms.and_then(|ms| Utc.timestamp_millis_opt(ms as i64).single());

        if let Some(oldest) = oldest_dt {
            let _ = update_backfill_cursor(pool, row.id, oldest, newest_dt, total_pages, total_bars).await;
            // Advance cursor for next block
            cursor_ms = Some((oldest.timestamp_millis() as u64).saturating_sub(1));
        }

        info!(
            engine_symbol_id = %row.id,
            symbol = %sym,
            interval = %iv,
            block = block_idx + 1,
            block_bars = res.upserted,
            total_bars,
            total_pages,
            reached_listing = res.reached_listing,
            "backfill: block done"
        );

        if res.reached_listing {
            info!(
                engine_symbol_id = %row.id,
                symbol = %sym,
                interval = %iv,
                total_bars,
                total_pages,
                "backfill: COMPLETE — reached listing date"
            );
            let _ = mark_backfill_finished(pool, row.id).await;
            return;
        }

        // If the block returned 0 bars, something is off — bail
        if res.upserted == 0 {
            warn!(engine_symbol_id = %row.id, symbol = %sym, "backfill: 0 bars in block, stopping");
            return;
        }
    }

    info!(
        engine_symbol_id = %row.id,
        symbol = %sym,
        interval = %iv,
        total_bars,
        "backfill: tick limit reached, will continue next tick"
    );
}

/// Phase 2: Verify data completeness after backfill.
async fn run_verification(
    pool: &PgPool,
    row: &EngineSymbolRow,
    gap_window: i64,
    exp: Option<i64>,
) {
    let ex = row.exchange.trim();
    let seg = segment_db(row.segment.trim());
    let sym = row.symbol.trim();
    let iv = row.interval.trim();

    let (count, min_ot, max_ot) = match count_market_bars_series(pool, ex, seg, sym, iv).await {
        Ok(x) => x,
        Err(e) => {
            warn!(%e, "verification: count_market_bars_series");
            return;
        }
    };

    // Calculate expected bar count from time span
    let expected = match (min_ot, max_ot, exp) {
        (Some(mn), Some(mx), Some(sec)) if sec > 0 => {
            let span = mx.signed_duration_since(mn).num_seconds();
            (span / sec) + 1
        }
        _ => count as i64,
    };

    // Gap scan — check ALL bars, not just recent window
    let mut gap_count = 0_i32;
    let mut max_gap_sec: Option<i32> = None;
    if let Some(sec) = exp {
        // Scan in chunks to avoid memory issues for large series
        let scan_limit = gap_window.max(count as i64).min(500_000);
        match list_recent_bar_open_times_desc(pool, ex, seg, sym, iv, scan_limit).await {
            Ok(mut desc) => {
                desc.reverse();
                let (g, mx) = scan_gaps_asc(&desc, sec);
                gap_count = g;
                max_gap_sec = mx;
            }
            Err(e) => warn!(%e, "verification gap scan"),
        }
    }

    // Completeness criteria:
    //   1. Bar count >= 95% of expected (some bars may legitimately not exist,
    //      e.g., exchange maintenance, zero-volume periods)
    //   2. No large gaps (max_gap < 10× interval)
    let ratio = if expected > 0 { count as f64 / expected as f64 } else { 1.0 };
    let large_gap = max_gap_sec
        .and_then(|g| exp.map(|s| g as i64 > s * 10))
        .unwrap_or(false);
    let is_complete = ratio >= 0.95 && !large_gap;

    info!(
        engine_symbol_id = %row.id,
        symbol = %row.symbol.trim(),
        interval = %iv,
        count,
        expected,
        ratio = format!("{:.2}%", ratio * 100.0),
        gap_count,
        max_gap_seconds = ?max_gap_sec,
        is_complete,
        "verification result"
    );

    let _ = update_verification(
        pool,
        row.id,
        count as i64,
        expected,
        gap_count,
        max_gap_sec,
        is_complete,
    )
    .await;

    // Also update ingestion state for GUI
    let _ = upsert_engine_symbol_ingestion_state(
        pool,
        row.id,
        count.clamp(0, i32::MAX as i64) as i32,
        min_ot,
        max_ot,
        gap_count,
        max_gap_sec,
        None,
        Utc::now(),
        if is_complete { None } else { Some("incomplete_data") },
    )
    .await;
}

/// Phase 3: Live updates — patch gaps, catch up stale feed.
async fn run_live_updates(
    pool: &PgPool,
    row: &EngineSymbolRow,
    _min_bars: i64,
    gap_window: i64,
    provider: &dyn MarketBarProvider,
    exp: Option<i64>,
) {
    let ex = row.exchange.trim();
    let seg = row.segment.trim();
    let sym = row.symbol.trim();
    let iv = row.interval.trim();
    let now = Utc::now();

    let (count, min_ot, max_ot) = match count_market_bars_series(pool, ex, segment_db(seg), sym, iv).await {
        Ok(x) => x,
        Err(_) => return,
    };

    // Promote complete → live on first successful live tick
    let _ = mark_live(pool, row.id).await;

    // Gap scan on trailing window
    let mut gap_n = 0_i32;
    let mut max_gap_sec: Option<i32> = None;
    if let Some(sec) = exp {
        if let Ok(mut desc) = list_recent_bar_open_times_desc(pool, ex, segment_db(seg), sym, iv, gap_window).await {
            desc.reverse();
            let (g, mx) = scan_gaps_asc(&desc, sec);
            gap_n = g;
            max_gap_sec = mx;
        }
    }

    // Stale tail check: catch up if latest bar is old
    let needs_patch = exp.and_then(|sec| {
        let stale_n = max_ot.map(|t| {
            ((now.signed_duration_since(t).num_seconds() / sec).max(0) + 50) as i64
        });
        let gap_n_fetch = if gap_n > 0 { Some(gap_window) } else { None };
        match (stale_n, gap_n_fetch) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
        .filter(|_| {
            max_ot
                .map(|t| now.signed_duration_since(t).num_seconds() > sec * 2)
                .unwrap_or(true)
                || gap_n > 0
        })
        .filter(|n| *n >= 10)
    })
    .map(|n| n.clamp(100, 15_000));

    let mut last_backfill: Option<DateTime<Utc>> = None;
    let mut last_err: Option<String> = None;

    if let Some(fetch_n) = needs_patch {
        match provider.backfill_bars(sym, iv, seg, fetch_n).await {
            Ok(n) => {
                last_backfill = Some(Utc::now());
                info!(
                    engine_symbol_id = %row.id,
                    symbol = %sym,
                    interval = %iv,
                    upserted = n,
                    gap_n,
                    "live: patch backfill (gap/stale)"
                );
            }
            Err(e) => {
                last_err = Some(format!("patch:{e}"));
                warn!(%e, engine_symbol_id = %row.id, "live patch backfill");
            }
        }
    }

    // Update ingestion state metrics
    let (count2, min_ot2, max_ot2) = match count_market_bars_series(pool, ex, segment_db(seg), sym, iv).await {
        Ok(x) => x,
        Err(_) => (count, min_ot, max_ot),
    };

    let mut err = last_err;
    if err.is_none() {
        if let (Some(maxt), Some(sec)) = (max_ot2, exp) {
            let age = now.signed_duration_since(maxt).num_seconds();
            if age > sec.saturating_mul(6) {
                err = Some(format!("stale_feed_seconds:{age}"));
            }
        }
    }

    let _ = upsert_engine_symbol_ingestion_state(
        pool,
        row.id,
        count2.clamp(0, i32::MAX as i64) as i32,
        min_ot2,
        max_ot2,
        gap_n,
        max_gap_sec,
        last_backfill,
        now,
        err.as_deref(),
    )
    .await;
}

/// Legacy cold-start path: for when full_history is disabled.
/// Uses simple min_bars threshold and non-resumable backfill.
async fn run_cold_start_and_live(
    pool: &PgPool,
    row: &EngineSymbolRow,
    min_bars: i64,
    gap_window: i64,
    provider: &dyn MarketBarProvider,
) {
    let ex = row.exchange.trim();
    let seg = row.segment.trim();
    let sym = row.symbol.trim();
    let iv = row.interval.trim();

    let (count, _, _) = match count_market_bars_series(pool, ex, segment_db(seg), sym, iv).await {
        Ok(x) => x,
        Err(_) => return,
    };

    if count < min_bars {
        let need = (min_bars - count).max(0) as i64;
        let fetch_n = need.saturating_add(200).clamp(300, 15_000);
        match provider.backfill_bars(sym, iv, seg, fetch_n).await {
            Ok(n) => {
                info!(
                    engine_symbol_id = %row.id,
                    symbol = %sym,
                    interval = %iv,
                    upserted = n,
                    "cold-start backfill"
                );
            }
            Err(e) => {
                warn!(%e, engine_symbol_id = %row.id, "cold-start backfill");
            }
        }
    }

    // Mark as complete (no full history verification needed)
    let _ = mark_backfill_finished(pool, row.id).await;
    let _ = update_verification(pool, row.id, count as i64, count as i64, 0, None, true).await;

    let exp = binance_kline_interval_seconds(iv);
    run_live_updates(pool, row, min_bars, gap_window, provider, exp).await;
}

// ─── Main Loop ──────────────────────────────────────────────────────

pub async fn engine_symbol_ingest_loop(pool: PgPool) {
    loop {
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "engine_ingest_tick_secs",
            "QTSS_ENGINE_INGEST_TICK_SECS",
            180,
            60,
        )
        .await;
        let min_bars = resolve_system_u64(
            &pool,
            "worker",
            "engine_ingest_min_bars",
            "QTSS_ENGINE_INGEST_MIN_BARS",
            2_000,
            120,
            50_000,
        )
        .await as i64;
        let gap_window = resolve_system_u64(
            &pool,
            "worker",
            "engine_ingest_gap_window",
            "QTSS_ENGINE_INGEST_GAP_WINDOW",
            2_000,
            100,
            20_000,
        )
        .await as i64;
        let full_history = resolve_system_u64(
            &pool,
            "worker",
            "engine_ingest_full_history",
            "QTSS_ENGINE_INGEST_FULL_HISTORY",
            1,  // default ON — fetch full history from listing date
            0,
            1,
        )
        .await == 1;

        // Run backfill for all symbols concurrently (bounded by semaphore
        // to stay within Binance rate limits — max 4 parallel fetchers).
        let sem = Arc::new(Semaphore::new(4));

        match list_enabled_engine_symbols(&pool).await {
            Ok(rows) => {
                // ── Phase A: verification + live (no API calls, no semaphore) ──
                for r in &rows {
                    let progress = match get_or_create_backfill_progress(&pool, r.id).await {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    match progress.state.as_str() {
                        "verifying" => {
                            let exp = binance_kline_interval_seconds(r.interval.trim());
                            run_verification(&pool, r, gap_window, exp).await;
                        }
                        "complete" | "live" => {
                            let prov = qtss_binance::BinanceBarProvider::new(pool.clone());
                            let exp = binance_kline_interval_seconds(r.interval.trim());
                            run_live_updates(&pool, r, min_bars, gap_window, &prov, exp).await;
                        }
                        _ => {} // pending/backfilling handled in Phase B
                    }
                }

                // ── Phase B: backfill (API calls, bounded by semaphore) ────────
                let mut handles = Vec::new();
                for r in rows {
                    let ex = r.exchange.trim().to_lowercase();
                    if ex != "binance" {
                        continue;
                    }
                    // Only spawn tasks for pending/backfilling — others already handled
                    let progress = match get_or_create_backfill_progress(&pool, r.id).await {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    if progress.state != "pending" && progress.state != "backfilling" {
                        continue;
                    }
                    let pool2 = pool.clone();
                    let sem2 = sem.clone();
                    handles.push(tokio::spawn(async move {
                        let _permit = sem2.acquire().await;
                        let prov = qtss_binance::BinanceBarProvider::new(pool2.clone());
                        run_one_target(&pool2, &r, min_bars, gap_window, full_history, &prov).await;
                    }));
                }
                for h in handles {
                    let _ = h.await;
                }
            }
            Err(e) => warn!(%e, "engine_ingest list_enabled_engine_symbols"),
        }
        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}
