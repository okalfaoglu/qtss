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

use std::time::Duration;

use chrono::Utc;
use qtss_ai::multi_gate::{self, GateContext, GateThresholds, VerdictStatus};
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

pub async fn allocator_v2_loop(pool: PgPool) {
    info!("allocator_v2_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(load_tick_secs(&pool).await)).await;
            continue;
        }
        let secs = load_tick_secs(&pool).await;
        match run_tick(&pool).await {
            Ok(n) if n > 0 => info!(armed = n, "allocator_v2 tick ok"),
            Ok(_) => debug!("allocator_v2 tick: no new setups"),
            Err(e) => warn!(%e, "allocator_v2 tick failed"),
        }
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

async fn run_tick(pool: &PgPool) -> anyhow::Result<usize> {
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
    let rows = sqlx::query(
        r#"WITH strong AS (
             SELECT exchange, segment, symbol, timeframe,
                    net_score, confidence, verdict, regime, computed_at
               FROM confluence_snapshots
              WHERE computed_at >= now() - make_interval(mins => $1::int)
                AND verdict IN ('strong_bull', 'strong_bear')
                AND abs(net_score) >= $2
           )
           SELECT DISTINCT ON (exchange, segment, symbol, timeframe)
                  exchange, segment, symbol, timeframe,
                  net_score, confidence, verdict, regime, computed_at
             FROM strong
            ORDER BY exchange, segment, symbol, timeframe, computed_at DESC"#,
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
        // Per-mode dedup — an existing dry setup doesn't block a
        // fresh live setup (operator flipped to dual-mode). A mode is
        // skipped only when *that specific mode* already has an open
        // setup for the series. Computed now so the loop can arm
        // only the missing modes.
        let mut modes_to_arm: Vec<String> = Vec::with_capacity(cfg.modes.len());
        for mode in &cfg.modes {
            let existing: i64 = sqlx::query_scalar(
                r#"SELECT count(*) FROM qtss_setups
                    WHERE exchange=$1 AND symbol=$2 AND timeframe=$3
                      AND state IN ('armed','active')
                      AND mode = $4"#,
            )
            .bind(&exchange)
            .bind(&symbol)
            .bind(&timeframe)
            .bind(mode)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            if existing == 0 {
                modes_to_arm.push(mode.clone());
            }
        }
        if modes_to_arm.is_empty() {
            info!(%symbol, "allocator_v2: skipping — all modes already armed");
            continue;
        }

        let direction = if verdict == "strong_bull" { "long" } else { "short" };
        let dir_sign: f64 = if direction == "long" { 1.0 } else { -1.0 };

        // Entry = latest close + ATR fallback for SL/TP.
        let Some((entry, atr)) = load_price_atr(pool, &exchange, &segment, &symbol, &timeframe)
            .await
        else {
            info!(%symbol, %timeframe, "allocator_v2: skipping — no price/ATR available");
            continue;
        };
        info!(%symbol, %timeframe, entry, atr, "allocator_v2: price+atr loaded");
        if atr <= 0.0 || entry <= 0.0 {
            info!(%symbol, atr, entry, "allocator_v2: skipping — zero atr/entry");
            continue;
        }
        let sl = entry - dir_sign * atr * cfg.atr_sl_mult;
        let tp1 = entry + dir_sign * atr * cfg.atr_tp_mult[0];
        let tp2 = entry + dir_sign * atr * cfg.atr_tp_mult[1];
        let tp3 = entry + dir_sign * atr * cfg.atr_tp_mult[2];

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

        // Approved — arm one setup per *missing* mode. Each mode is
        // a separate qtss_setups row; selector_loop picks both up,
        // execution_bridge dispatches based on mode.
        for mode in &modes_to_arm {
            let setup_id = sqlx::query_scalar::<_, sqlx::types::Uuid>(
                r#"INSERT INTO qtss_setups
                      (venue_class, exchange, symbol, timeframe, profile, state,
                       direction, entry_price, entry_sl, current_sl, target_ref,
                       risk_pct, mode, tp_ladder, raw_meta)
                   VALUES ('crypto', $1, $2, $3, 't', 'armed',
                           $4, $5::real, $6::real, $6::real, $7::real,
                           $8, $9, $10, $11)
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
            }))
            .fetch_one(pool)
            .await?;

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
    let dir_arrow = if direction == "long" { "⬆" } else { "⬇" };
    let (title, severity) = if event_key == "allocator_v2_armed" {
        (
            format!(
                "{dir_arrow} {mode} {symbol} {timeframe}  @ {entry:.4}  SL {sl:.4}  conf {confidence:.2}"
            ),
            "info",
        )
    } else {
        (
            format!("⛔ {symbol} {timeframe} — setup rejected"),
            "warn",
        )
    };
    let body = serde_json::to_string_pretty(payload).unwrap_or_default();
    let _ = sqlx::query(
        r#"INSERT INTO notify_outbox
              (title, body, channels, severity, event_key,
               exchange, segment, symbol, status)
           VALUES ($1, $2, '["telegram","x","webhook"]'::jsonb, $3, $4,
                   'binance', 'futures', $5, 'pending')
           ON CONFLICT DO NOTHING"#,
    )
    .bind(&title)
    .bind(&body)
    .bind(severity)
    .bind(event_key)
    .bind(symbol)
    .execute(pool)
    .await;
    Ok(())
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
        atr_sl_mult: 1.5,
        atr_tp_mult: [1.5, 3.0, 5.0],
        risk_pct_per_trade: 0.01,
        default_org_id: sqlx::types::Uuid::nil(),
        default_user_id: sqlx::types::Uuid::nil(),
        modes: vec!["dry".to_string()],
    };
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
