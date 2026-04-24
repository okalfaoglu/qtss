// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `allocator_v2_loop` — bridges the new detection stack (Faz 11-13)
//! to the existing dry-trade pipeline (qtss_setups → selected_candidates
//! → live_positions via execution_bridge).
//!
//! Tick flow:
//!   1. Scan `confluence_snapshots` — find symbols where
//!      `|net_score| >= min` AND verdict ∈ {strong_bull, strong_bear}
//!      in the last N minutes.
//!   2. Skip rows already represented by an armed/active qtss_setup.
//!   3. Build entry / SL / TP ladder from latest bar + ATR fallback.
//!   4. Run AI multi-gate (qtss-ai::multi_gate) for approval.
//!   5. Approved → INSERT qtss_setups(state='armed', mode='dry') +
//!      notify_outbox row for Telegram lifecycle handler.
//!   6. selector_loop picks it up → execution_bridge opens a dry
//!      position in `live_positions` → setup_watcher tracks TP/SL.
//!
//! Rejected setups land in `ai_approval_requests` with the gate scores
//! so the operator / chart can see WHY a signal didn't fire.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Datelike, Timelike, Utc};
use qtss_ai::multi_gate::{self, GateContext, GateThresholds, VerdictStatus};
use qtss_notify::PriceTickStore;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

pub async fn allocator_v2_loop(pool: PgPool, price_store: Arc<PriceTickStore>) {
    info!("allocator_v2_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(load_tick_secs(&pool).await)).await;
            continue;
        }
        let secs = load_tick_secs(&pool).await;
        // Setup v1.1.2 — warm-up gate. On a fresh start (or after a DB
        // nuke) the bookTicker stream hasn't finished its first connect,
        // so `price_store.len()` reads 0 and the allocator would fall
        // through to `bar_close_no_stream` for every candidate. Those
        // bar-based entries are the very bug we just fixed, so skip
        // the tick entirely until at least `warmup_min_subscribers`
        // symbols have live ticks. Throttled log so operators can tell
        // warm-up from a long outage.
        let cfg_peek = load_cfg(&pool).await;
        let have = price_store.len();
        if have < cfg_peek.warmup_min_subscribers {
            info!(
                tick_store_size = have,
                need = cfg_peek.warmup_min_subscribers,
                "allocator_v2: waiting for bookTicker warm-up"
            );
            tokio::time::sleep(Duration::from_secs(secs.min(15))).await;
            continue;
        }
        match run_tick(&pool, &price_store).await {
            Ok(n) if n > 0 => info!(armed = n, "allocator_v2 tick ok"),
            Ok(_) => debug!("allocator_v2 tick: no new setups"),
            Err(e) => warn!(%e, "allocator_v2 tick failed"),
        }
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

async fn run_tick(pool: &PgPool, price_store: &PriceTickStore) -> anyhow::Result<usize> {
    let cfg = load_cfg(pool).await;
    // Latest confluence row per (symbol, timeframe) in the lookback
    // window with a strong verdict. DISTINCT ON picks the most recent
    // snapshot per series.
    // Pick the MOST RECENT strong_* snapshot per (series) in the
    // lookback window — not the most recent overall. If the latest
    // tick for a series is weak_* or mixed, we shouldn't auto-trade;
    // but if within the lookback window there was a strong_* signal
    // AND the current reading hasn't flipped direction, we act on the
    // last strong reading. Implementation: filter first, THEN
    // distinct-on.
    // Setup v1.1.4 — feed the candidates in HTF-first order. The
    // per-profile dedup relies on a higher-TF candidate winning the
    // slot over a lower-TF one; if candidates arrived in arbitrary
    // order an LTF setup could arm first, and as soon as the HTF
    // candidate processed it would retire the LTF — causing a
    // thrash that re-opens the LTF every time the HTF flickers.
    // Processing HTF first means: the first candidate to land is
    // always the "right" one; everything below just skips.
    let rows = sqlx::query(
        r#"WITH strong AS (
             SELECT exchange, segment, symbol, timeframe,
                    net_score, confidence, verdict, regime, computed_at
               FROM confluence_snapshots
              WHERE computed_at >= now() - make_interval(mins => $1::int)
                AND verdict IN ('strong_bull', 'strong_bear')
                AND abs(net_score) >= $2
           ),
           distinct_strong AS (
             SELECT DISTINCT ON (exchange, segment, symbol, timeframe)
                    exchange, segment, symbol, timeframe,
                    net_score, confidence, verdict, regime, computed_at
               FROM strong
              ORDER BY exchange, segment, symbol, timeframe, computed_at DESC
           )
           SELECT exchange, segment, symbol, timeframe,
                  net_score, confidence, verdict, regime, computed_at
             FROM distinct_strong
            ORDER BY
              symbol,
              CASE timeframe
                WHEN '1M' THEN 43200
                WHEN '1w' THEN 10080
                WHEN '1d' THEN 1440
                WHEN '12h' THEN 720
                WHEN '8h'  THEN 480
                WHEN '6h'  THEN 360
                WHEN '4h'  THEN 240
                WHEN '2h'  THEN 120
                WHEN '1h'  THEN 60
                WHEN '30m' THEN 30
                WHEN '15m' THEN 15
                WHEN '5m'  THEN 5
                WHEN '3m'  THEN 3
                WHEN '1m'  THEN 1
                ELSE 60
              END DESC"#,
    )
    .bind(cfg.lookback_minutes as i32)
    .bind(cfg.min_abs_net_score)
    .fetch_all(pool)
    .await?;

    info!(
        candidates = rows.len(),
        lookback_minutes = cfg.lookback_minutes,
        min_abs_net_score = cfg.min_abs_net_score,
        "allocator_v2: candidates fetched"
    );
    let mut armed = 0usize;
    for r in rows {
        let exchange: String = r.get("exchange");
        let segment: String = r.get("segment");
        let symbol: String = r.get("symbol");
        let timeframe: String = r.get("timeframe");
        let net_score: f64 = r.try_get("net_score").unwrap_or(0.0);
        let confidence: f64 = r.try_get("confidence").unwrap_or(0.0);
        let verdict: String = r.try_get("verdict").unwrap_or_default();
        let regime: Option<String> = r.try_get("regime").ok();

        info!(%symbol, %timeframe, %verdict, %net_score, "allocator_v2: processing candidate");

        let direction = if verdict == "strong_bull" { "long" } else { "short" };
        let dir_sign: f64 = if direction == "long" { 1.0 } else { -1.0 };
        let profile = tf_profile(&timeframe);
        let new_bar_mins = tf_bar_minutes(&timeframe);

        // Setup v1.1.3 — per-profile HTF dedup.
        // For each mode: is there already an armed/active setup on the
        // same (symbol, direction, profile)? If YES and its TF is >=
        // the new TF, skip (HTF wins). If its TF is < new TF, retire
        // the lower-TF setup (rejected_reason=upgraded_to_higher_tf_
        // within_profile) so the higher-TF one can arm in its place.
        // Different profiles (T vs D) never block each other — they
        // track different time horizons.
        let mut modes_to_arm: Vec<String> = Vec::with_capacity(cfg.modes.len());
        let mut superseded: Vec<sqlx::types::Uuid> = Vec::new();
        for mode in &cfg.modes {
            let existing: Vec<(sqlx::types::Uuid, String)> = sqlx::query_as(
                r#"SELECT id, timeframe FROM qtss_setups
                    WHERE exchange = $1
                      AND symbol = $2
                      AND profile = $3
                      AND direction = $4
                      AND state IN ('armed','active')
                      AND mode = $5"#,
            )
            .bind(&exchange)
            .bind(&symbol)
            .bind(profile)
            .bind(direction)
            .bind(mode)
            .fetch_all(pool)
            .await
            .unwrap_or_default();
            let mut keep_existing = false;
            for (eid, etf) in &existing {
                let existing_mins = tf_bar_minutes(etf);
                if existing_mins >= new_bar_mins {
                    // An equal-or-higher TF setup already holds the
                    // (symbol, direction, profile, mode) slot.
                    keep_existing = true;
                } else {
                    // New TF is the HTF here — retire the lower one.
                    superseded.push(*eid);
                }
            }
            if !keep_existing {
                modes_to_arm.push(mode.clone());
            }
        }
        // Setup v1.1.5 — the previous revision retired the lower-TF
        // setups up-front, before the HTF candidate actually cleared
        // the price/ATR/commission/sanity checks. If the HTF couldn't
        // be armed (e.g. market_bars for 1w was empty), the lower TF
        // was destroyed *and* no replacement went in — on the next
        // allocator tick the LTF re-armed, and the HTF candidate
        // retired it again. Stuck loop, observed live as
        // BTCUSDT/ETHUSDT 1d setups cycling every 60 seconds.
        //
        // Hold the retirement until the HTF candidate has actually
        // opened its replacement: "supersede-on-success". If any of
        // the later gates fail we just continue — the LTF stays
        // armed until a healthy HTF candidate comes along.
        if modes_to_arm.is_empty() {
            // Nothing to arm here (the LTF candidate slot belongs to
            // an equal-or-higher TF that's already armed). Leave the
            // current holder alone and move on.
            info!(%symbol, %timeframe, %profile, "allocator_v2: skipping — higher/equal TF already armed");
            continue;
        }

        // Entry sourcing — Setup v1.1: prefer the live bookTicker price
        // (side-aware: short executes against best_bid, long against
        // best_ask) over the last bar close, because the bar is up to
        // one TF-length stale. ATR is still derived from the bar series
        // — it's a volatility estimate, not an executable price.
        let Some((bar_close, atr)) = load_price_atr(
            pool, &exchange, &segment, &symbol, &timeframe,
        )
        .await
        else {
            info!(%symbol, %timeframe, "allocator_v2: skipping — no price/ATR available");
            continue;
        };
        if atr <= 0.0 || bar_close <= 0.0 {
            info!(%symbol, atr, bar_close, "allocator_v2: skipping — zero atr/bar_close");
            continue;
        }
        // Pull the fresh tick; fall back to bar close if the bookTicker
        // stream hasn't seen this symbol yet (e.g. fresh install or
        // WS disconnected). Record the entry_source in raw_meta so the
        // Setups GUI can tell a tick-backed entry from a bar-fallback.
        let (entry, entry_source) = match price_store.get("binance", &symbol) {
            Some(tick) => {
                // Side-aware: buying crosses the ask, selling crosses
                // the bid. That matches what the broker will actually
                // fill at, so entry reflects real executable price.
                let side_px = if direction == "long" {
                    tick.ask
                } else {
                    tick.bid
                };
                let px = side_px
                    .to_f64()
                    .or_else(|| tick.mid().to_f64());
                match px.filter(|p| *p > 0.0) {
                    Some(p) => (p, "live_tick"),
                    None => (bar_close, "bar_close_fallback"),
                }
            }
            None => (bar_close, "bar_close_no_stream"),
        };
        info!(
            %symbol, %timeframe, entry, bar_close, atr, entry_source,
            "allocator_v2: price+atr loaded"
        );
        // v1.1.9 — structure-aware SL.
        // `atr_sl` is the volatility-based stop. If a recent opposing
        // swing pivot sits further from entry than atr_sl, respect
        // the structure (with a factor < 1.0 so we don't put SL at
        // the exact swing — a hair inside to avoid exact-pivot
        // sniping). If the swing is closer than atr_sl we still use
        // atr_sl so the SL doesn't go BELOW/ABOVE atr-expected noise.
        let atr_sl_dist = atr * cfg.atr_sl_mult;
        let struct_dist: Option<f64> = if cfg.structure_sl_enabled {
            nearest_opposing_swing_distance(
                pool, &exchange, &segment, &symbol, &timeframe, direction, entry,
            )
            .await
        } else {
            None
        };
        let effective_sl_dist = match struct_dist {
            Some(d) if d > atr_sl_dist => d * cfg.structure_sl_factor,
            _ => atr_sl_dist,
        };
        if let Some(d) = struct_dist {
            debug!(
                %symbol, %timeframe, atr_sl_dist, struct_dist = d,
                effective_sl_dist,
                "allocator_v2: structure-aware SL computed"
            );
        }
        let sl = entry - dir_sign * effective_sl_dist;
        let tp1 = entry + dir_sign * atr * cfg.atr_tp_mult[0];
        let tp2 = entry + dir_sign * atr * cfg.atr_tp_mult[1];
        let tp3 = entry + dir_sign * atr * cfg.atr_tp_mult[2];

        // MTF opposing-direction gate — scoped to the same profile.
        // Setup v1.1.3: D-long (1d macro uptrend) + T-short (15m
        // pullback short) is a legitimate cross-horizon pair — should
        // not block each other. But T-long + T-short (or D-long +
        // D-short) inside the same profile is self-hedging and gets
        // gated.
        if cfg.mtf_opposing_gate_enabled {
            let opp: i64 = sqlx::query_scalar(
                r#"SELECT count(*)
                     FROM qtss_setups
                    WHERE exchange = $1
                      AND symbol = $2
                      AND profile = $3
                      AND state IN ('armed','active')
                      AND direction <> $4"#,
            )
            .bind(&exchange)
            .bind(&symbol)
            .bind(profile)
            .bind(direction)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            if opp > 0 {
                info!(
                    %symbol, direction, %profile,
                    "allocator_v2: skip — opposing-direction setup already armed in same profile"
                );
                continue;
            }
        }

        // Cooldown guard — if the same (symbol, direction) took an SL
        // hit inside the last N minutes, skip. This kills the whipsaw
        // loop we saw live on XRPUSDT where an identical entry/SL pair
        // was re-armed every 60 seconds only to get stopped out a few
        // seconds later.
        let cooldown_min = cfg.sl_hit_cooldown_minutes;
        if cooldown_min > 0 {
            let recent_loss: i64 = sqlx::query_scalar(
                r#"SELECT count(*)
                     FROM qtss_setups
                    WHERE exchange = $1 AND symbol = $2 AND direction = $3
                      AND close_reason = 'sl_hit'
                      AND closed_at >= now() - make_interval(mins => $4::int)"#,
            )
            .bind(&exchange)
            .bind(&symbol)
            .bind(direction)
            .bind(cooldown_min as i32)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            if recent_loss > 0 {
                info!(
                    %symbol, direction, cooldown_min, recent_loss,
                    "allocator_v2: skip — same-direction sl_hit within cooldown"
                );
                continue;
            }
        }

        // v1.1.6 — loss-streak cooldown (extended). After N consecutive
        // sl_hit closes on the same (symbol, direction), ban for M
        // minutes. Catches the "3 losses in a row, the system is wrong
        // about this symbol right now" anti-pattern.
        if cfg.loss_streak_threshold > 0 && cfg.loss_streak_ban_minutes > 0 {
            let streak: i64 = sqlx::query_scalar(
                r#"WITH recent AS (
                      SELECT close_reason, closed_at
                        FROM qtss_setups
                       WHERE exchange = $1 AND symbol = $2 AND direction = $3
                         AND closed_at IS NOT NULL
                         AND close_reason IN ('sl_hit','tp_final','trail_stop',
                                              'invalidated','cancelled')
                         AND close_reason NOT IN ('cancelled')
                         AND closed_at >= now() - make_interval(mins => $4::int)
                       ORDER BY closed_at DESC
                       LIMIT 20
                   )
                   SELECT count(*)::bigint FROM recent
                    WHERE close_reason = 'sl_hit'"#,
            )
            .bind(&exchange)
            .bind(&symbol)
            .bind(direction)
            .bind(cfg.loss_streak_ban_minutes as i32)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            if streak >= cfg.loss_streak_threshold {
                info!(
                    %symbol, direction, streak,
                    threshold = cfg.loss_streak_threshold,
                    ban = cfg.loss_streak_ban_minutes,
                    "allocator_v2: skip — loss-streak ban active"
                );
                continue;
            }
        }

        // v1.1.6 — correlation-cluster gate. "BTC long + ETH long +
        // SOL long" = one macro idea fired three times. Group crypto
        // into coarse clusters and cap concurrent armed per
        // (cluster, direction). Clusters intentionally broad — the
        // goal is to stop naive multi-symbol duplication, not to model
        // precise correlation.
        if cfg.corr_cluster_enabled && cfg.corr_cluster_max_armed > 0 {
            let my_cluster = symbol_cluster(&symbol);
            if let Some(my_cluster) = my_cluster {
                // Fetch sibling symbols in the cluster and count armed
                // setups in the SAME direction.
                let siblings = cluster_symbols(my_cluster);
                let armed_in_cluster: i64 = sqlx::query_scalar(
                    r#"SELECT count(*) FROM qtss_setups
                        WHERE exchange = $1
                          AND symbol = ANY($2)
                          AND direction = $3
                          AND state IN ('armed','active')"#,
                )
                .bind(&exchange)
                .bind(&siblings)
                .bind(direction)
                .fetch_one(pool)
                .await
                .unwrap_or(0);
                if armed_in_cluster >= cfg.corr_cluster_max_armed {
                    info!(
                        %symbol, direction,
                        cluster = my_cluster,
                        armed_in_cluster,
                        max = cfg.corr_cluster_max_armed,
                        "allocator_v2: skip — correlation cluster cap reached"
                    );
                    continue;
                }
            }
        }

        // v1.1.6 — daily armed cap across ALL symbols. Hard brake on
        // over-trading regardless of symbol distribution. Skip gate
        // when max_daily_armed = 0 (operator off).
        if cfg.max_daily_armed > 0 {
            let armed_today: i64 = sqlx::query_scalar(
                r#"SELECT count(*) FROM qtss_setups
                    WHERE created_at >= now() - interval '24 hours'
                      AND state IN ('armed','active')"#,
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            if armed_today >= cfg.max_daily_armed {
                info!(
                    %symbol, armed_today,
                    cap = cfg.max_daily_armed,
                    "allocator_v2: skip — daily armed cap reached"
                );
                continue;
            }
        }

        // v1.1.6 — EV gate. Historical (symbol, direction, profile)
        // expected-value-in-R: if negative (and we have >= ev_min_sample
        // closed trades to be confident), refuse the candidate.
        // Sample-size guard keeps the system alive during cold start.
        if cfg.ev_gate_enabled {
            let profile_peek = tf_profile(&timeframe);
            let stats: Option<(i64, f64, f64)> = sqlx::query_as(
                r#"SELECT
                      count(*) FILTER (WHERE realized_pnl_pct IS NOT NULL)::bigint
                        AS closed_n,
                      COALESCE(AVG(CASE WHEN realized_pnl_pct > 0 THEN realized_pnl_pct END), 0)::float8
                        AS avg_win_pct,
                      COALESCE(AVG(CASE WHEN realized_pnl_pct < 0 THEN abs(realized_pnl_pct) END), 0)::float8
                        AS avg_loss_pct
                     FROM qtss_setups
                    WHERE symbol = $1
                      AND direction = $2
                      AND profile = $3
                      AND closed_at IS NOT NULL
                      AND close_reason IN ('tp_final','sl_hit','trail_stop','invalidated')
                      AND realized_pnl_pct IS NOT NULL"#,
            )
            .bind(&symbol)
            .bind(direction)
            .bind(profile_peek)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
            if let Some((closed_n, avg_win_pct, avg_loss_pct)) = stats {
                if closed_n >= cfg.ev_min_sample {
                    let winrate: f64 = sqlx::query_scalar(
                        r#"SELECT (count(*) FILTER (WHERE realized_pnl_pct > 0)::float8 /
                                   NULLIF(count(*)::float8, 0))
                             FROM qtss_setups
                            WHERE symbol = $1
                              AND direction = $2
                              AND profile = $3
                              AND closed_at IS NOT NULL
                              AND realized_pnl_pct IS NOT NULL"#,
                    )
                    .bind(&symbol)
                    .bind(direction)
                    .bind(profile_peek)
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0.5);
                    let ev_pct =
                        winrate * avg_win_pct - (1.0 - winrate) * avg_loss_pct;
                    if ev_pct < cfg.ev_min_value_r {
                        info!(
                            %symbol, direction, profile = profile_peek,
                            closed_n, winrate, avg_win_pct, avg_loss_pct, ev_pct,
                            min_ev = cfg.ev_min_value_r,
                            "allocator_v2: skip — negative expected value"
                        );
                        continue;
                    }
                }
            }
        }

        // v1.1.7 — HTF context gate. ChatGPT flagged this as the single
        // highest-ROI addition: a 15m long against a 1d strong_bear is
        // fighting the trend. If any HTF in the lookup set is strong_*
        // in the OPPOSITE direction, skip. Absence of an HTF strong_*
        // snapshot is treated as "no disagreement" (neutral HTF is not
        // a veto).
        if cfg.htf_context_gate_enabled {
            let htfs = htf_lookup_set(&timeframe);
            if !htfs.is_empty() {
                let opposite_verdict = if direction == "long" {
                    "strong_bear"
                } else {
                    "strong_bull"
                };
                let lookback_mins =
                    tf_bar_minutes(&timeframe).max(60) * 4; // at least 4 bars of the candidate TF
                let conflicting_htf: Option<String> = sqlx::query_scalar(
                    r#"SELECT timeframe FROM confluence_snapshots
                        WHERE exchange = $1
                          AND segment = $2
                          AND symbol = $3
                          AND timeframe = ANY($4)
                          AND verdict = $5
                          AND computed_at >= now() - make_interval(mins => $6::int)
                        ORDER BY computed_at DESC
                        LIMIT 1"#,
                )
                .bind(&exchange)
                .bind(&segment)
                .bind(&symbol)
                .bind(&htfs)
                .bind(opposite_verdict)
                .bind(lookback_mins as i32)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();
                if let Some(htf) = conflicting_htf {
                    info!(
                        %symbol, direction, candidate_tf = %timeframe,
                        conflicting_htf = %htf,
                        opposite_verdict,
                        "allocator_v2: skip — HTF contradicts candidate direction"
                    );
                    continue;
                }
            }
        }

        // Count today's rejections for this symbol (for gate 5).
        let rejected_today: i64 = sqlx::query_scalar(
            r#"SELECT count(*) FROM ai_approval_requests
                WHERE status = 'rejected'
                  AND (payload->>'symbol') = $1
                  AND created_at >= now() - interval '24 hours'"#,
        )
        .bind(&symbol)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        // AI multi-gate.
        let thr = load_gate_thresholds(pool).await;
        let ctx = GateContext {
            symbol: symbol.clone(),
            confidence,
            meta_label: None, // No ML model loaded yet — gate bypassed.
            regime: regime.clone().unwrap_or_else(|| "uncertain".into()),
            confluence: confidence,
            rejected_today,
            in_event_blackout: false, // Macro blackout — PR-16 will populate.
        };
        let verdict_out = multi_gate::evaluate(&ctx, &thr);
        let gate_scores = multi_gate::gate_scores_json(&verdict_out);
        let payload = json!({
            "symbol": symbol,
            "exchange": exchange,
            "segment": segment,
            "timeframe": timeframe,
            "verdict": verdict,
            "net_score": net_score,
            "confidence": confidence,
            "regime": regime,
            "direction": direction,
            "entry": entry,
            "sl": sl,
            "tp_ladder": [tp1, tp2, tp3],
            "atr": atr,
        });

        // Always create the ai_approval audit row — approved +
        // pending + rejected all land there for the Config Editor +
        // Telegram card to read.
        let approval_id = match sqlx::query_scalar::<_, sqlx::types::Uuid>(
            r#"INSERT INTO ai_approval_requests
                  (org_id, requester_user_id, status, kind, payload,
                   gate_scores, rejection_reason, auto_approved)
               VALUES ($1, $2, $3, 'setup_v2_autonomous', $4, $5, $6, $7)
               RETURNING id"#,
        )
        .bind(cfg.default_org_id)
        .bind(cfg.default_user_id)
        .bind(verdict_out.verdict.as_str())
        .bind(&payload)
        .bind(&gate_scores)
        .bind(verdict_out.rejection_reason.map(|r| r.as_str().to_string()))
        .bind(verdict_out.auto_approved)
        .fetch_one(pool)
        .await
        {
            Ok(id) => id,
            Err(e) => {
                warn!(%e, %symbol, "allocator_v2: ai_approval insert failed");
                continue;
            }
        };

        if !matches!(verdict_out.verdict, VerdictStatus::Approved) {
            // Not auto-approved — log + let the pending row ride.
            // Reject notifications also land on Telegram via the same
            // outbox handler (payload.kind distinguishes).
            let _ = insert_notify_outbox(
                pool,
                "allocator_v2_rejected",
                &json!({
                    "approval_id": approval_id,
                    "symbol": symbol,
                    "timeframe": timeframe,
                    "reason": verdict_out.rejection_reason.map(|r| r.as_str().to_string()),
                    "gates": gate_scores,
                }),
            )
            .await;
            continue;
        }

        // Commission-viability pre-check — before we commit capital to
        // an arm, verify the gross TP move comfortably outruns round-trip
        // taker commission. If the first take-profit can't even cover
        // `safety_multiple × round_trip_pct`, the setup is a negative-
        // expectancy trade net of fees. Write it with state='rejected'
        // anyway so operators see exactly why the pipeline refused —
        // otherwise the decision is invisible.
        let round_trip_pct = (cfg.commission_taker_bps * 2.0) / 100.0; // bps → pct, ×2 legs
        let gross_tp1_pct = ((tp1 - entry).abs() / entry.max(1e-9)) * 100.0;
        let commission_viable = gross_tp1_pct
            >= cfg.commission_safety_multiple * round_trip_pct;
        let commission_check = json!({
            "gross_tp1_pct": gross_tp1_pct,
            "round_trip_pct": round_trip_pct,
            "taker_bps": cfg.commission_taker_bps,
            "safety_multiple": cfg.commission_safety_multiple,
            "threshold_pct": cfg.commission_safety_multiple * round_trip_pct,
            "viable": commission_viable,
        });

        // Pre-trade sanity — even with a live-tick entry, the spot price
        // at insert time may already be past the SL (fast market, gap).
        // For long: live must be <= SL is a nonsense state (we'd open
        // short-of-SL). Same mirror for short. When the tick stream is
        // unavailable we still check against bar_close; fail-open for
        // fresh installs is preferable to silent staleness.
        let reference_px = match price_store
            .get("binance", &symbol)
            .and_then(|t| t.mid().to_f64())
        {
            Some(p) if p > 0.0 => p,
            _ => bar_close,
        };
        let sanity_viable = if direction == "long" {
            reference_px < sl_guard_floor(entry, sl)
                && reference_px > sl
        } else {
            reference_px > sl_guard_ceiling(entry, sl)
                && reference_px < sl
        };
        let sanity_check = json!({
            "reference_price": reference_px,
            "entry": entry,
            "sl": sl,
            "direction": direction,
            "viable": sanity_viable,
            "source": entry_source,
        });

        // Setup v1.1.2 — minimum SL distance. 15m bars can produce SL
        // distances as tight as 0.3% of entry; a single 1-candle wick
        // blows through them before the bar even closes. We require
        // `sl_distance_pct >= sl_min_distance_pct` (default 0.4% — about
        // 4× round-trip commission) so the allocator refuses to open
        // "noise-level" trades. Not a bar/TF-specific guard because
        // the same logic applies everywhere: if your SL is inside
        // typical intra-bar noise, you're going to get stopped every
        // bar by variance alone.
        let sl_distance_pct = ((entry - sl).abs() / entry.max(1e-9)) * 100.0;
        let sl_distance_viable = sl_distance_pct >= cfg.sl_min_distance_pct;
        let sl_distance_check = json!({
            "sl_distance_pct": sl_distance_pct,
            "min_required_pct": cfg.sl_min_distance_pct,
            "viable": sl_distance_viable,
        });

        let viable = commission_viable && sanity_viable && sl_distance_viable;

        // If any gate failed, write the row for audit but never arm it.
        let setup_state = if viable { "armed" } else { "rejected" };
        let setup_close_reason: Option<&str> = if viable { None } else { Some("cancelled") };
        let setup_rejected_reason: Option<&str> = if viable {
            None
        } else if !commission_viable {
            Some("gross_tp_below_commission_floor")
        } else if !sanity_viable {
            Some("live_price_crossed_sl_at_open")
        } else {
            Some("sl_too_tight")
        };

        // Approved — arm one setup per *missing* mode. Each mode is
        // a separate qtss_setups row; selector_loop picks both up,
        // execution_bridge dispatches based on mode.
        // Setup v1.1.3 — profile is derived from the TF, not hard-coded.
        // T (5m/15m/30m) = intraday trades; D (1h/4h/1d/1w) = swing.
        let profile = tf_profile(&timeframe);

        for mode in &modes_to_arm {
            let setup_id = sqlx::query_scalar::<_, sqlx::types::Uuid>(
                r#"INSERT INTO qtss_setups
                      (venue_class, exchange, symbol, timeframe, profile, state,
                       direction, entry_price, entry_sl, current_sl, target_ref,
                       risk_pct, mode, tp_ladder, raw_meta,
                       close_reason, closed_at, ai_score)
                   VALUES ('crypto', $1, $2, $3, $14, $12,
                           $4, $5::real, $6::real, $6::real, $7::real,
                           $8, $9, $10, $11,
                           $13,
                           CASE WHEN $12 = 'rejected' THEN now() ELSE NULL END,
                           $15::real)
                   RETURNING id"#,
            )
            .bind(&exchange)
            .bind(&symbol)
            .bind(&timeframe)
            .bind(direction)
            .bind(entry as f32)
            .bind(sl as f32)
            .bind(tp1 as f32)
            .bind(cfg.risk_pct_per_trade as f32)
            .bind(mode)
            .bind(json!([
                {"idx": 1, "price": tp1, "size_pct": 0.5},
                {"idx": 2, "price": tp2, "size_pct": 0.3},
                {"idx": 3, "price": tp3, "size_pct": 0.2}
            ]))
            .bind(json!({
                "source": "allocator_v2",
                "approval_id": approval_id,
                "net_score": net_score,
                "confidence": confidence,
                "regime": regime,
                "verdict": verdict,
                "atr": atr,
                "gate_scores": gate_scores,
                "mode": mode,
                "entry_source": entry_source,
                "bar_close": bar_close,
                "commission_check": commission_check,
                "sanity_check": sanity_check,
                "sl_distance_check": sl_distance_check,
                "rejected_reason": setup_rejected_reason
                    .map(|s| serde_json::Value::String(s.to_string()))
                    .unwrap_or(serde_json::Value::Null),
            }))
            .bind(setup_state)
            .bind(setup_close_reason)
            .bind(profile)
            .bind(confidence as f32)
            .fetch_one(pool)
            .await?;

            // Supersede-on-success. Only retire the lower-TF holders
            // once the HTF candidate actually went in as armed. A
            // rejected candidate (commission / sanity / sl_too_tight)
            // does NOT dethrone its predecessor — otherwise the churn
            // loop we fixed in Setup v1.1.5 re-opens (LTF retired,
            // HTF couldn't take the slot, next tick LTF re-arms).
            if viable && !superseded.is_empty() {
                for id in &superseded {
                    let _ = sqlx::query(
                        r#"UPDATE qtss_setups
                              SET state = 'rejected',
                                  close_reason = 'cancelled',
                                  closed_at = now(),
                                  updated_at = now(),
                                  raw_meta = raw_meta ||
                                             jsonb_build_object('rejected_reason',
                                                                'upgraded_to_higher_tf_within_profile')
                            WHERE id = $1 AND state IN ('armed','active')"#,
                    )
                    .bind(id)
                    .execute(pool)
                    .await;
                    let _ = sqlx::query(
                        r#"UPDATE live_positions
                              SET closed_at    = now(),
                                  updated_at   = now(),
                                  close_reason = 'setup_superseded',
                                  realized_pnl_quote = COALESCE(
                                      (last_mark - entry_avg) * qty_remaining *
                                      CASE WHEN side = 'BUY' THEN 1 ELSE -1 END,
                                      0
                                  )
                            WHERE setup_id = $1 AND closed_at IS NULL"#,
                    )
                    .bind(id)
                    .execute(pool)
                    .await;
                }
                info!(
                    %symbol, %timeframe, %direction, %profile,
                    superseded_count = superseded.len(),
                    "allocator_v2: retired lower-TF setups after HTF armed"
                );
            }

            if !viable {
                // Surface the skip to ops so we don't need to poll the
                // DB — the outbox handler fans out to Telegram.
                let event_key = if !commission_viable {
                    "allocator_v2_commission_skip"
                } else if !sanity_viable {
                    "allocator_v2_sanity_skip"
                } else {
                    "allocator_v2_sl_too_tight"
                };
                let _ = insert_notify_outbox(
                    pool,
                    event_key,
                    &json!({
                        "setup_id": setup_id,
                        "symbol": symbol,
                        "timeframe": timeframe,
                        "mode": mode,
                        "commission_check": commission_check,
                        "sanity_check": sanity_check,
                        "sl_distance_check": sl_distance_check,
                    }),
                )
                .await;
                continue; // skip the "armed" outbox below for this mode
            }

            let _ = insert_notify_outbox(
                pool,
                "allocator_v2_armed",
                &json!({
                    "setup_id": setup_id,
                    "symbol": symbol,
                    "timeframe": timeframe,
                    "direction": direction,
                    "entry": entry,
                    "sl": sl,
                    "tp_ladder": [tp1, tp2, tp3],
                    "net_score": net_score,
                    "confidence": confidence,
                    "verdict": verdict,
                    "regime": regime,
                    "mode": mode,
                    "commission_check": commission_check,
                }),
            )
            .await;

            armed += 1;
        }
    }
    Ok(armed)
}

async fn load_price_atr(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    timeframe: &str,
) -> Option<(f64, f64)> {
    let bar = sqlx::query(
        r#"SELECT close FROM market_bars
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND interval=$4
            ORDER BY open_time DESC LIMIT 1"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    let close = bar
        .try_get::<rust_decimal::Decimal, _>("close")
        .ok()?
        .to_f64()?;
    let atr_row = sqlx::query(
        r#"SELECT values FROM indicator_snapshots
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4 AND indicator='atr'
            ORDER BY bar_time DESC LIMIT 1"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let atr = atr_row
        .and_then(|r| r.try_get::<Value, _>("values").ok())
        .and_then(|v| v.get("atr").and_then(|x| x.as_f64()))
        .unwrap_or(0.0);
    Some((close, atr))
}

async fn insert_notify_outbox(
    pool: &PgPool,
    event_key: &str,
    payload: &Value,
) -> anyhow::Result<()> {
    // dedup_key is event_key + setup_id — after a worker restart,
    // re-inserting the same card for an already-processed setup is a
    // no-op (ON CONFLICT DO NOTHING via the partial unique index on
    // notify_outbox.dedup_key). Prevents the "same Telegram card fires
    // every restart" regression.
    let setup_id_str = payload
        .get("setup_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // For `allocator_v2_armed` only, widen the dedup to
    // (symbol, direction, profile_bucket, 15-minute bucket) so a
    // dedup-churn cycle (HTF pops in, pops out, LTF re-arms) does
    // NOT spam Telegram with a fresh "armed" card every 60 seconds.
    // Setup-id-only dedup is fine for the other event kinds because
    // they're by-setup audit trails, not ops-facing signals.
    let dedup_key: Option<String> = if event_key == "allocator_v2_armed" {
        let sym = payload.get("symbol").and_then(|v| v.as_str()).unwrap_or("?");
        let dir = payload.get("direction").and_then(|v| v.as_str()).unwrap_or("?");
        let tf = payload.get("timeframe").and_then(|v| v.as_str()).unwrap_or("?");
        let profile_bucket = match tf {
            "5m" | "15m" | "30m" => "t",
            _ => "d",
        };
        let mode = payload.get("mode").and_then(|v| v.as_str()).unwrap_or("?");
        // 15-minute bucket — four Telegram buckets per hour max. If
        // the underlying setup stays armed longer the hourly snapshot
        // already keeps the user informed; we don't need another
        // "armed" card.
        let now = Utc::now();
        let min_bucket = (now.minute() / 15) * 15;
        let bucket = format!(
            "{:04}{:02}{:02}{:02}{:02}",
            now.year(),
            now.month(),
            now.day(),
            now.hour(),
            min_bucket
        );
        Some(format!(
            "armed:{sym}:{dir}:{profile_bucket}:{mode}:{bucket}"
        ))
    } else {
        setup_id_str
            .as_ref()
            .map(|sid| format!("{event_key}:{sid}"))
    };
    // Schema match: notify_outbox(title, body, channels, severity,
    // event_key, org_id, exchange, segment, symbol, status).
    // Errors swallowed — a broken outbox should never crash the
    // allocator's hot path.
    let symbol = payload.get("symbol").and_then(|v| v.as_str()).unwrap_or("?");
    let timeframe = payload.get("timeframe").and_then(|v| v.as_str()).unwrap_or("?");
    let direction = payload.get("direction").and_then(|v| v.as_str()).unwrap_or("?");
    let mode = payload.get("mode").and_then(|v| v.as_str()).unwrap_or("?");
    let entry = payload.get("entry").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let sl = payload.get("sl").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let confidence = payload.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let dir_arrow = if direction == "long" { "🟢" } else { "🔴" };
    let (title, severity, body) = if event_key == "allocator_v2_armed" {
        (
            format!(
                "{dir_arrow} {mode} {symbol} {timeframe}  @ {entry:.4}  SL {sl:.4}  conf {confidence:.2}"
            ),
            "info",
            render_armed_html(payload),
        )
    } else if event_key == "allocator_v2_commission_skip" {
        (
            format!("🟡 {symbol} {timeframe} — komisyon filtresi"),
            "warn",
            render_commission_skip_html(payload),
        )
    } else {
        (
            format!("⛔ {symbol} {timeframe} — setup rejected"),
            "warn",
            serde_json::to_string_pretty(payload).unwrap_or_default(),
        )
    };
    // Channel routing:
    //   * armed setup → Telegram (operators want the live alert).
    //   * reject / skip events → audit only via webhook; they're
    //     transparency rows, Telegram feed should not be spammed with
    //     every filter-trip.
    //   * Everything else (future event kinds) → Telegram by default
    //     so a new event shows up without a code change here.
    let channels = match event_key {
        "allocator_v2_armed" => r#"["telegram"]"#,
        "allocator_v2_commission_skip"
        | "allocator_v2_sanity_skip"
        | "allocator_v2_sl_too_tight" => r#"["webhook"]"#,
        _ => r#"["telegram"]"#,
    };
    let _ = sqlx::query(
        r#"INSERT INTO notify_outbox
              (title, body, channels, severity, event_key,
               exchange, segment, symbol, status, dedup_key)
           VALUES ($1, $2, $7::jsonb, $3, $4,
                   'binance', 'futures', $5, 'pending', $6)
           ON CONFLICT (dedup_key) WHERE dedup_key IS NOT NULL DO NOTHING"#,
    )
    .bind(&title)
    .bind(&body)
    .bind(severity)
    .bind(event_key)
    .bind(symbol)
    .bind(dedup_key.as_deref())
    .bind(channels)
    .execute(pool)
    .await;
    Ok(())
}

// ── Telegram HTML renderers ────────────────────────────────────────────
//
// notify_outbox_loop treats a body as Telegram HTML when it starts with
// `<b>`/`<i>`/`<pre>` — that's what `parse_mode=HTML` needs in the
// Telegram API. Non-Telegram channels (email, webhook) still get a
// plaintext version because the loop strips the tags for them.

/// Format a signed percentage with leading sign.
fn fmt_pct_signed(p: f64) -> String {
    if p.abs() < 1e-9 {
        "0.00%".to_string()
    } else if p >= 0.0 {
        format!("+{p:.2}%")
    } else {
        format!("{p:.2}%")
    }
}

/// Distance from entry to `other` as a signed percentage (raw, no
/// direction awareness). Positive = `other` > entry.
fn pct_raw(entry: f64, other: f64) -> f64 {
    if entry.abs() < 1e-9 {
        return 0.0;
    }
    (other - entry) / entry * 100.0
}

fn render_armed_html(p: &Value) -> String {
    let symbol = p.get("symbol").and_then(|v| v.as_str()).unwrap_or("?");
    let tf = p.get("timeframe").and_then(|v| v.as_str()).unwrap_or("?");
    let direction = p.get("direction").and_then(|v| v.as_str()).unwrap_or("?");
    let mode = p.get("mode").and_then(|v| v.as_str()).unwrap_or("?");
    let verdict = p.get("verdict").and_then(|v| v.as_str()).unwrap_or("?");
    let entry = p.get("entry").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let sl = p.get("sl").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let confidence = p
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let tp_ladder: Vec<f64> = p
        .get("tp_ladder")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default();
    let is_long = direction == "long";
    let dir_icon = if is_long { "🟢 LONG" } else { "🔴 SHORT" };

    // SL/TP moves relative to entry — signs flip for shorts so "+%" is
    // always the favourable direction.
    let sl_pct_raw = pct_raw(entry, sl);
    let sl_pct_display = if is_long { sl_pct_raw } else { -sl_pct_raw };

    // Commission — reconstruct round-trip from the embedded check block
    // if the allocator already computed it; else skip the commission
    // section entirely. Body stays honest rather than hard-coding bps.
    let commission = p.get("commission_check");
    let (round_trip_pct, gross_tp1_pct) = commission
        .map(|c| {
            (
                c.get("round_trip_pct").and_then(|v| v.as_f64()).unwrap_or(0.0),
                c.get("gross_tp1_pct").and_then(|v| v.as_f64()).unwrap_or(0.0),
            )
        })
        .unwrap_or((0.0, 0.0));

    let mut html = String::with_capacity(1024);
    html.push_str(&format!(
        "<b>{dir_icon}</b>  <b>{symbol}</b> · <code>{tf}</code>  <i>({mode})</i>\n\n"
    ));
    html.push_str(&format!(
        "<b>Durum:</b> {verdict} · AI <b>{confidence:.2}</b>/1.00\n"
    ));
    html.push_str(&format!("<b>Giriş:</b>    <code>{entry:.6}</code>\n"));
    html.push_str(&format!(
        "<b>Stop (SL):</b> <code>{sl:.6}</code>  ({})\n",
        fmt_pct_signed(sl_pct_display)
    ));

    if !tp_ladder.is_empty() {
        html.push_str("\n<b>Kâr Al (TP):</b>\n");
        for (i, tp) in tp_ladder.iter().enumerate() {
            let raw = pct_raw(entry, *tp);
            let tp_pct = if is_long { raw } else { -raw };
            html.push_str(&format!(
                "  <b>TP{}:</b> <code>{:.6}</code>  ({})\n",
                i + 1,
                tp,
                fmt_pct_signed(tp_pct)
            ));
        }
        // Gross R:R from TP1 / SL.
        if !tp_ladder.is_empty() && sl_pct_display.abs() > 1e-9 {
            let tp1 = tp_ladder[0];
            let tp1_raw = pct_raw(entry, tp1);
            let tp1_pct = if is_long { tp1_raw } else { -tp1_raw };
            let rr = (tp1_pct / sl_pct_display).abs();
            html.push_str(&format!("\n<b>Risk : Ödül</b>  1 : {rr:.2}\n"));
        }
    }

    if round_trip_pct > 0.0 && gross_tp1_pct > 0.0 {
        let net_tp1 = gross_tp1_pct - round_trip_pct;
        html.push_str(&format!(
            "\n<b>Komisyon (round trip):</b> {round_trip_pct:.3}%\n"
        ));
        html.push_str(&format!(
            "<b>Net TP1 (fees sonrası):</b> {net_tp1:+.3}%\n"
        ));
    }

    html
}

fn render_commission_skip_html(p: &Value) -> String {
    let symbol = p.get("symbol").and_then(|v| v.as_str()).unwrap_or("?");
    let tf = p.get("timeframe").and_then(|v| v.as_str()).unwrap_or("?");
    let mode = p.get("mode").and_then(|v| v.as_str()).unwrap_or("?");
    let c = p.get("commission_check").cloned().unwrap_or(Value::Null);
    let gross = c.get("gross_tp1_pct").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let rt = c.get("round_trip_pct").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let thr = c
        .get("threshold_pct")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let bps = c.get("taker_bps").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let mult = c
        .get("safety_multiple")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    format!(
        "<b>🟡 {symbol}</b> · <code>{tf}</code>  <i>({mode})</i>\n\
         <b>Durum:</b> setup üretildi fakat <b>komisyon filtresine takıldı</b>\n\n\
         <b>Gross TP1:</b>    {gross:.3}%\n\
         <b>Round trip:</b>    {rt:.3}%  ({bps:.1} bps × 2)\n\
         <b>Threshold:</b>     {thr:.3}%  (round trip × {mult:.1})\n\n\
         <i>TP hedefi komisyonu karşılayamadığı için işlem açılmadı. \
         Setup durumu <b>rejected</b> olarak kaydedildi, \
         raw_meta.rejected_reason = <code>gross_tp_below_commission_floor</code>.</i>"
    )
}

// ── Config ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Cfg {
    lookback_minutes: i64,
    min_abs_net_score: f64,
    atr_sl_mult: f64,
    atr_tp_mult: [f64; 3],
    risk_pct_per_trade: f64,
    default_org_id: sqlx::types::Uuid,
    default_user_id: sqlx::types::Uuid,
    /// Which modes to arm per approved signal. Typical:
    ///   ["dry"]         — paper-only (default, zero risk)
    ///   ["dry","live"]  — parallel paper + real execution. Also
    ///                     requires `execution.execution.live.enabled
    ///                     = true` (execution_bridge gate) AND valid
    ///                     `exchange_accounts` rows.
    modes: Vec<String>,
    /// Round-trip commission viability gate — a setup is rejected when
    /// its first take-profit does not reach `safety_multiple × round-
    /// trip taker commission` above entry. Default 2.0 (TP must be at
    /// least 2× round-trip fees). Reading the bps directly here rather
    /// than via qtss-fees keeps the allocator self-contained; the
    /// system_config row is shared with the Setup drawer (/v2/fees).
    commission_taker_bps: f64,
    commission_safety_multiple: f64,
    /// Setup v1.1 — after an SL hit on (symbol, direction), suppress
    /// new arms for this many minutes. 0 disables the guard; default
    /// 15 kills the XRPUSDT-style whipsaw loop without blocking a
    /// legitimate regime flip from rearming at the reverse direction.
    sl_hit_cooldown_minutes: i64,
    /// Setup v1.1.1 — when true, the allocator refuses to arm a new
    /// setup on a symbol that already has an armed setup in the
    /// opposite direction. Stops the pipeline from self-hedging when
    /// 15m says strong_bear and 1d says strong_bull simultaneously.
    mtf_opposing_gate_enabled: bool,
    /// Setup v1.1.2 — skip allocator ticks until the bookTicker
    /// stream has at least this many live symbols. Zero disables the
    /// warm-up gate (legacy behaviour); default 3 blocks the cold-
    /// start fiasco where the first tick opens every setup at stale
    /// bar_close fallback and then the live tick immediately stops
    /// them out.
    warmup_min_subscribers: usize,
    /// Setup v1.1.2 — minimum SL distance in percent of entry. Setups
    /// with SL tighter than this are rejected so the allocator doesn't
    /// open trades inside typical intra-bar noise. 0.4% ≈ 4 × round-
    /// trip taker commission is the default.
    sl_min_distance_pct: f64,

    // v1.1.6 — quality-over-quantity pack.
    //
    /// Max setups armed in the last 24h across ALL symbols. Hard brake
    /// on over-trading. 0 disables.
    max_daily_armed: i64,
    /// Loss-streak cooldown. After N consecutive sl_hit closes on the
    /// same (symbol, direction), ban for M minutes. Separate from the
    /// v1.1 single-SL 15min cooldown — this catches the "system going
    /// blind" anti-pattern ChatGPT flagged.
    loss_streak_threshold: i64,
    loss_streak_ban_minutes: i64,
    /// Correlation cluster gate — groups BTC/ETH or the L1s or the
    /// memes together so "all longs" doesn't fire N parallel trades
    /// on one macro idea. When more than `corr_cluster_max_armed`
    /// setups in the same cluster-direction are already armed, the
    /// new candidate is skipped.
    corr_cluster_enabled: bool,
    corr_cluster_max_armed: i64,
    /// EV gate — refuse a setup whose historical (symbol, direction,
    /// profile) expected-value-in-R is < 0. `ev_min_sample` closes
    /// required before the gate can take action; below the threshold
    /// the system stays in "learning" mode and skips the gate to
    /// avoid cold-start paralysis.
    ev_gate_enabled: bool,
    ev_min_sample: i64,
    ev_min_value_r: f64,

    /// v1.1.7 — HTF context gate. A lower-TF setup (e.g. 15m long)
    /// must not contradict its higher-TF confluence verdict (e.g. 1d
    /// strong_bear). The gate queries the opposite-direction strong_*
    /// snapshot on the HTF lookup set for the candidate's TF and
    /// rejects the candidate when an HTF disagrees.
    htf_context_gate_enabled: bool,

    /// v1.1.9 — structure-aware SL. Instead of `sl = entry - atr`,
    /// allow the SL to honour the nearest opposing pivot (swing) if
    /// that point is further from entry than the ATR estimate —
    /// `sl = entry - max(atr * atr_sl_mult, struct_dist * factor)`.
    /// This stops the stop from sitting INSIDE the last swing low/
    /// high, where noise regularly trades through. `factor` is the
    /// proportion of the swing distance honoured (0.8 default keeps
    /// SL just inside the structure to avoid exact-pivot sniping).
    structure_sl_enabled: bool,
    structure_sl_factor: f64,
}

// Floor/ceiling sanity-check bounds. For a long, the live price must
// sit between SL (below) and something sane above entry (here: entry +
// 2× the entry-SL distance, generous enough to allow mid-bar drift
// without letting the allocator open wildly above its planned entry).
// These are fn's, not constants, because they depend on entry+sl.
fn sl_guard_floor(entry: f64, sl: f64) -> f64 {
    let d = (entry - sl).abs();
    entry + d * 2.0 // for longs, never open above this ceiling either
}

fn sl_guard_ceiling(entry: f64, sl: f64) -> f64 {
    let d = (entry - sl).abs();
    entry - d * 2.0 // for shorts, never open below this floor either
}

// v1.1.6 — coarse correlation buckets for the cluster-cap gate.
// Matches on the base symbol prefix so future USDT/USDC variants fall
// into the same bucket. Expand as new venues/symbols come online.
fn symbol_cluster(symbol: &str) -> Option<&'static str> {
    let s = symbol.to_ascii_uppercase();
    // Strip the quote for matching — BTC / BTCUSDT / BTCUSDC all map
    // to the same cluster.
    let base = s
        .strip_suffix("USDT")
        .or_else(|| s.strip_suffix("USDC"))
        .or_else(|| s.strip_suffix("USD"))
        .unwrap_or(&s);
    match base {
        "BTC" | "ETH" => Some("majors"),
        "SOL" | "AVAX" | "ADA" | "DOT" | "NEAR" | "APT" | "SUI" | "BNB" => Some("l1s"),
        "DOGE" | "SHIB" | "PEPE" | "FLOKI" | "WIF" | "BONK" => Some("memes"),
        "LINK" | "UNI" | "AAVE" | "MKR" | "COMP" | "ARB" | "OP" => Some("defi"),
        "XRP" | "LTC" | "TRX" | "XLM" => Some("payments"),
        _ => None,
    }
}

/// Return every symbol (with USDT suffix) that sits in the given
/// cluster. Used by the cluster-cap gate's count query via ANY($2).
// v1.1.9 — Distance from `entry` to the nearest opposing-direction
// swing pivot on (symbol, tf). "Opposing" means: for a long candidate
// we look for a SWING LOW below entry (so SL below swing is the
// natural stop); for a short candidate we look for a SWING HIGH
// above entry. Joins `pivots` via engine_symbols to resolve
// (exchange, segment, symbol, interval). Pivot.direction convention:
// +1 = swing high, -1 = swing low (also accepts ±2 for strong swings).
//
// Returns None when there's no recent pivot on the correct side.
async fn nearest_opposing_swing_distance(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    timeframe: &str,
    direction: &str,
    entry: f64,
) -> Option<f64> {
    // For a long: want the nearest SWING LOW below entry (direction = -1).
    // For a short: want the nearest SWING HIGH above entry (direction = +1).
    let target_dir: i16 = if direction == "long" { -1 } else { 1 };
    let row = sqlx::query(
        r#"SELECT p.price::float8 AS price
             FROM pivots p
             JOIN engine_symbols e ON e.id = p.engine_symbol_id
            WHERE e.exchange = $1
              AND e.segment = $2
              AND e.symbol = $3
              AND e.interval = $4
              AND sign(p.direction::int) = $5::int
              AND CASE WHEN $5::int = -1
                       THEN p.price::float8 < $6
                       ELSE p.price::float8 > $6
                  END
              AND p.open_time >= now() - interval '30 days'
            ORDER BY p.open_time DESC
            LIMIT 1"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .bind(target_dir as i32)
    .bind(entry)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    let px: f64 = row.try_get("price").ok()?;
    let d = (entry - px).abs();
    if d.is_finite() && d > 0.0 {
        Some(d)
    } else {
        None
    }
}

// v1.1.7 — HTFs to consult for the context gate when the candidate
// arrives on the given TF. Three or more HTFs return the candidate
// is usually fine on intraday; higher TFs have progressively fewer
// confirming sources (macro is lonely at the top).
fn htf_lookup_set(tf: &str) -> Vec<&'static str> {
    match tf {
        "1m" | "3m" | "5m" => vec!["15m", "30m", "1h"],
        "15m" => vec!["1h", "4h"],
        "30m" => vec!["1h", "4h"],
        "1h" => vec!["4h", "1d"],
        "4h" => vec!["1d", "1w"],
        "1d" => vec!["1w"],
        _ => vec![],
    }
}

fn cluster_symbols(cluster: &str) -> Vec<String> {
    let bases: &[&str] = match cluster {
        "majors" => &["BTC", "ETH"],
        "l1s" => &["SOL", "AVAX", "ADA", "DOT", "NEAR", "APT", "SUI", "BNB"],
        "memes" => &["DOGE", "SHIB", "PEPE", "FLOKI", "WIF", "BONK"],
        "defi" => &["LINK", "UNI", "AAVE", "MKR", "COMP", "ARB", "OP"],
        "payments" => &["XRP", "LTC", "TRX", "XLM"],
        _ => &[],
    };
    bases.iter().map(|b| format!("{b}USDT")).collect()
}

// Setup v1.1.3 — TF → profile mapping.
//
// T (Trade, short-horizon): intraday entries tracking a few hours to
//   maybe a day. Fast TP/SL cycle.
// D (Daily, long-horizon): swing / macro setups tracking days to
//   weeks. Wider SL, longer hold.
//
// One (symbol × direction × profile × mode) can have a single armed
// setup at a time — within the same profile the highest TF wins. T and
// D can both be armed at once because they represent different time
// perspectives and can legitimately coexist (D: "weekly uptrend",
// T: "hourly pullback short against the trend").
fn tf_profile(tf: &str) -> &'static str {
    match tf {
        "5m" | "15m" | "30m" => "t",
        "1h" | "4h" | "1d" | "3d" | "1w" => "d",
        _ => "d",
    }
}

/// Bar length in minutes — used to rank TFs within a profile so HTF
/// wins the dedup race. Unknown TFs fall back to 60 (neutral).
fn tf_bar_minutes(tf: &str) -> i64 {
    match tf {
        "1m" => 1,
        "3m" => 3,
        "5m" => 5,
        "15m" => 15,
        "30m" => 30,
        "1h" => 60,
        "2h" => 120,
        "4h" => 240,
        "6h" => 360,
        "8h" => 480,
        "12h" => 720,
        "1d" => 1440,
        "3d" => 4320,
        "1w" => 10080,
        _ => 60,
    }
}

async fn load_f64(pool: &PgPool, module: &str, key: &str) -> Option<f64> {
    let row = sqlx::query("SELECT value FROM system_config WHERE module = $1 AND config_key = $2")
        .bind(module)
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()?;
    let val: Value = row.try_get("value").ok()?;
    // Accept both `5.0` (bare number) and `{"value": 5}` shapes.
    match &val {
        Value::Number(n) => n.as_f64(),
        other => other.get("value").and_then(|v| v.as_f64()),
    }
}

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'allocator_v2' AND config_key = 'enabled'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return false; }; // OFF by default — explicit opt-in.
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false)
}

async fn load_tick_secs(pool: &PgPool) -> u64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'allocator_v2' AND config_key = 'tick_secs'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 60; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(60)
        .max(15)
}

async fn load_cfg(pool: &PgPool) -> Cfg {
    let mut cfg = Cfg {
        lookback_minutes: 10,
        min_abs_net_score: 1.5,
        // Defaults tuned for RR ≈ 2.0 on TP1 (see migration 0243).
        // Overridable per-row in system_config.allocator_v2.atr_*_mult.
        atr_sl_mult: 1.0,
        atr_tp_mult: [2.0, 3.5, 5.5],
        risk_pct_per_trade: 0.01,
        default_org_id: sqlx::types::Uuid::nil(),
        default_user_id: sqlx::types::Uuid::nil(),
        modes: vec!["dry".to_string()],
        commission_taker_bps: 5.0, // futures default; overridable
        commission_safety_multiple: 2.0,
        sl_hit_cooldown_minutes: 15,
        mtf_opposing_gate_enabled: true,
        warmup_min_subscribers: 3,
        sl_min_distance_pct: 0.4,
        // v1.1.6 defaults — tuneable via system_config.
        max_daily_armed: 10,
        loss_streak_threshold: 3,
        loss_streak_ban_minutes: 60,
        corr_cluster_enabled: true,
        corr_cluster_max_armed: 2,
        ev_gate_enabled: true,
        ev_min_sample: 10,
        ev_min_value_r: 0.0,
        htf_context_gate_enabled: true,
        structure_sl_enabled: true,
        structure_sl_factor: 0.8,
    };
    // Pull the current taker bps so the check matches what the Setup
    // drawer displays. Prefer binance_futures (live trading venue).
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config
           WHERE module = 'setup'
             AND config_key = 'commission.binance_futures.taker_bps'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        // Seeds store bare numbers (5.0) and {"value": 5} — accept both.
        let v = match &val {
            Value::Number(n) => n.as_f64(),
            other => other.get("value").and_then(|x| x.as_f64()),
        };
        if let Some(bps) = v {
            cfg.commission_taker_bps = bps.max(0.0);
        }
    }
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config
           WHERE module = 'allocator_v2'
             AND config_key = 'commission.safety_multiple'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(x) = val.get("value").and_then(|v| v.as_f64()) {
            cfg.commission_safety_multiple = x.max(1.0);
        }
    }
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config
           WHERE module = 'allocator_v2'
             AND config_key = 'sl_hit_cooldown_minutes'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(v) = val.get("value").and_then(|v| v.as_i64()) {
            cfg.sl_hit_cooldown_minutes = v.max(0);
        }
    }
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'allocator_v2' AND config_key = 'lookback_minutes'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(v) = val.get("value").and_then(|v| v.as_i64()) {
            cfg.lookback_minutes = v.max(1);
        }
    }
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'allocator_v2' AND config_key = 'min_abs_net_score'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(v) = val.get("value").and_then(|v| v.as_f64()) {
            cfg.min_abs_net_score = v;
        }
    }
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'allocator_v2' AND config_key = 'risk_pct_per_trade'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(v) = val.get("value").and_then(|v| v.as_f64()) {
            cfg.risk_pct_per_trade = v.max(0.0001).min(0.2);
        }
    }
    // Setup v1.1.1 — ATR multiples now live in system_config so
    // operators can retune RR without a redeploy.
    if let Some(v) = load_f64(pool, "allocator_v2", "atr_sl_mult").await {
        cfg.atr_sl_mult = v.max(0.1);
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "atr_tp_mult_0").await {
        cfg.atr_tp_mult[0] = v.max(0.1);
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "atr_tp_mult_1").await {
        cfg.atr_tp_mult[1] = v.max(0.1);
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "atr_tp_mult_2").await {
        cfg.atr_tp_mult[2] = v.max(0.1);
    }
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config
           WHERE module = 'allocator_v2'
             AND config_key = 'mtf_opposing_gate_enabled'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(b) = val.get("enabled").and_then(|v| v.as_bool()) {
            cfg.mtf_opposing_gate_enabled = b;
        }
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "warmup_min_subscribers").await {
        cfg.warmup_min_subscribers = v.max(0.0) as usize;
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "sl_min_distance_pct").await {
        cfg.sl_min_distance_pct = v.max(0.0);
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "max_daily_armed").await {
        cfg.max_daily_armed = v.max(0.0) as i64;
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "loss_streak_threshold").await {
        cfg.loss_streak_threshold = v.max(0.0) as i64;
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "loss_streak_ban_minutes").await {
        cfg.loss_streak_ban_minutes = v.max(0.0) as i64;
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "corr_cluster_max_armed").await {
        cfg.corr_cluster_max_armed = v.max(0.0) as i64;
    }
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config WHERE module='allocator_v2' AND config_key='corr_cluster_enabled'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(b) = val.get("enabled").and_then(|v| v.as_bool()) {
            cfg.corr_cluster_enabled = b;
        }
    }
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config WHERE module='allocator_v2' AND config_key='ev_gate_enabled'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(b) = val.get("enabled").and_then(|v| v.as_bool()) {
            cfg.ev_gate_enabled = b;
        }
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "ev_min_sample").await {
        cfg.ev_min_sample = v.max(1.0) as i64;
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "ev_min_value_r").await {
        cfg.ev_min_value_r = v;
    }
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config WHERE module='allocator_v2' AND config_key='htf_context_gate_enabled'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(b) = val.get("enabled").and_then(|v| v.as_bool()) {
            cfg.htf_context_gate_enabled = b;
        }
    }
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config WHERE module='allocator_v2' AND config_key='structure_sl_enabled'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(b) = val.get("enabled").and_then(|v| v.as_bool()) {
            cfg.structure_sl_enabled = b;
        }
    }
    if let Some(v) = load_f64(pool, "allocator_v2", "structure_sl_factor").await {
        cfg.structure_sl_factor = v.clamp(0.1, 1.5);
    }
    // Default org_id from dry-execution config.
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'dry' AND config_key = 'default_org_id'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(s) = val.get("value").and_then(|v| v.as_str()) {
            if let Ok(u) = sqlx::types::Uuid::parse_str(s) {
                cfg.default_org_id = u;
            }
        }
    }
    // Fallback: pick first organization in the DB.
    if cfg.default_org_id == sqlx::types::Uuid::nil() {
        if let Ok(Some(row)) = sqlx::query("SELECT id FROM organizations LIMIT 1")
            .fetch_optional(pool)
            .await
        {
            if let Ok(id) = row.try_get::<sqlx::types::Uuid, _>("id") {
                cfg.default_org_id = id;
            }
        }
    }
    // Fallback user_id: pick any admin/system user. The ai_approval_
    // requests.requester_user_id FK refuses NULL, so we need a valid
    // UUID even for autonomous writes.
    if cfg.default_user_id == sqlx::types::Uuid::nil() {
        if let Ok(Some(row)) = sqlx::query("SELECT id FROM users ORDER BY created_at LIMIT 1")
            .fetch_optional(pool)
            .await
        {
            if let Ok(id) = row.try_get::<sqlx::types::Uuid, _>("id") {
                cfg.default_user_id = id;
            }
        }
    }
    // Load modes list — dry-only by default, operator can add "live".
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'allocator_v2' AND config_key = 'modes'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(arr) = val.get("modes").and_then(|v| v.as_array()) {
            let modes: Vec<String> = arr
                .iter()
                .filter_map(|x| x.as_str().map(|s| s.to_ascii_lowercase()))
                .filter(|m| m == "dry" || m == "live" || m == "backtest")
                .collect();
            if !modes.is_empty() {
                cfg.modes = modes;
            }
        }
    }
    cfg
}

async fn load_gate_thresholds(pool: &PgPool) -> GateThresholds {
    let mut thr = GateThresholds::default();
    let rows = sqlx::query(
        r#"SELECT config_key, value FROM system_config
            WHERE module = 'ai_approval' AND (config_key = 'auto_approve_threshold'
                                             OR config_key LIKE 'gates.%')"#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for r in rows {
        let key: String = r.try_get("config_key").unwrap_or_default();
        let val: Value = r.try_get("value").unwrap_or(Value::Null);
        match key.as_str() {
            "auto_approve_threshold" => {
                if let Some(v) = val.get("value").and_then(|v| v.as_f64()) {
                    thr.auto_approve_threshold = v;
                }
            }
            "gates.min_confidence" => {
                if let Some(v) = val.get("value").and_then(|v| v.as_f64()) {
                    thr.min_confidence = v;
                }
            }
            "gates.min_meta_label" => {
                if let Some(v) = val.get("value").and_then(|v| v.as_f64()) {
                    thr.min_meta_label = v;
                }
            }
            "gates.min_confluence" => {
                if let Some(v) = val.get("value").and_then(|v| v.as_f64()) {
                    thr.min_confluence = v;
                }
            }
            "gates.max_daily_rejected_per_symbol" => {
                if let Some(v) = val.get("value").and_then(|v| v.as_i64()) {
                    thr.max_daily_rejected_per_symbol = v;
                }
            }
            "gates.event_blackout_minutes" => {
                if let Some(v) = val.get("value").and_then(|v| v.as_i64()) {
                    thr.event_blackout_minutes = v;
                }
            }
            "gates.regime_blacklist" => {
                if let Some(arr) = val.get("regimes").and_then(|v| v.as_array()) {
                    thr.regime_blacklist = arr
                        .iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect();
                }
            }
            _ => {}
        }
    }
    thr
}

// Keep `Utc` used so the ICE workaround for dead-code doesn't decide
// to flag the import. (Chrono is used indirectly through sqlx's
// timestamptz mapping.)
#[allow(unused)]
fn _keep_utc_used() -> chrono::DateTime<chrono::Utc> {
    Utc::now()
}
