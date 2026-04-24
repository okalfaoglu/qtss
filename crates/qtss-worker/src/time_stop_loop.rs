// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `time_stop_loop` — edge-decay exit (v1.1.8).
//!
//! ChatGPT teardown #7 + edge-decay rule: a setup whose TP has not
//! hit within a reasonable number of bars is probably in chop or
//! lost its edge. Rather than grinding down to SL, close it with
//! `close_reason='time_stop'` and free the capital.
//!
//! Tick every 60s. For each open (armed/active) setup whose
//! `tp1_hit=false`:
//!   * Compute age_bars = floor((now - created_at) / bar_minutes(tf))
//!   * If age_bars >= profile-specific max, close:
//!       - qtss_setups.state='closed_scratch' + close_reason='time_stop'
//!       - live_positions.closed_at=now(), realized_pnl_quote = MTM
//!   * Emit allocator_v2_time_stop notify_outbox row for operator.
//!
//! All thresholds live in `system_config.allocator_v2.time_stop.*`.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

pub async fn time_stop_loop(pool: PgPool) {
    info!("time_stop_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        match run_tick(&pool).await {
            Ok(n) if n > 0 => info!(closed = n, "time_stop_loop tick"),
            Ok(_) => debug!("time_stop_loop: nothing to close"),
            Err(e) => warn!(%e, "time_stop_loop tick failed"),
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='allocator_v2' AND config_key='time_stop.enabled'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return true; }; // opt-out
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

/// Per-profile maximum age (in bars of the candidate TF) before a
/// setup without a TP1 hit gets force-closed.
async fn max_bars(pool: &PgPool, profile: &str) -> i64 {
    let key = format!("time_stop.max_bars_{profile}");
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module='allocator_v2' AND config_key=$1",
    )
    .bind(&key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    if let Some(row) = row {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        let v = match &val {
            Value::Number(n) => n.as_i64(),
            other => other.get("value").and_then(|v| v.as_i64()),
        };
        if let Some(v) = v {
            return v.max(1);
        }
    }
    // Sensible defaults — T profile is short-horizon so 12 bars ≈
    // 3 hours on 15m; D profile runs longer so 24 bars ≈ a day on 1h
    // or four days on 4h.
    match profile {
        "t" => 12,
        _ => 24,
    }
}

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

async fn run_tick(pool: &PgPool) -> anyhow::Result<usize> {
    let t_max = max_bars(pool, "t").await;
    let d_max = max_bars(pool, "d").await;
    let rows = sqlx::query(
        r#"SELECT id, symbol, timeframe, direction, profile, created_at,
                  tp1_hit, entry_price
             FROM qtss_setups
            WHERE state IN ('armed','active')
              AND closed_at IS NULL
              AND COALESCE(tp1_hit, false) = false"#,
    )
    .fetch_all(pool)
    .await?;
    let mut closed = 0usize;
    for r in rows {
        let id: sqlx::types::Uuid = r.try_get("id")?;
        let symbol: String = r.try_get("symbol").unwrap_or_default();
        let timeframe: String = r.try_get("timeframe").unwrap_or_default();
        let profile: String = r.try_get("profile").unwrap_or_else(|_| "d".into());
        let created_at: DateTime<Utc> = r.try_get("created_at").unwrap_or_else(|_| Utc::now());
        let bar_mins = tf_bar_minutes(&timeframe).max(1);
        let age_mins = (Utc::now() - created_at).num_minutes();
        let age_bars = age_mins / bar_mins;
        let cap = match profile.as_str() {
            "t" => t_max,
            _ => d_max,
        };
        if age_bars < cap {
            continue;
        }
        info!(
            %symbol, %timeframe, %profile, age_bars, cap,
            "time_stop_loop: closing setup by edge decay"
        );
        // Close setup — 'closed_scratch' is the DB-allowed state that
        // best describes "timed out without a TP1 hit". close_reason
        // 'time_stop' is in the allowed close_reason enum.
        let _ = sqlx::query(
            r#"UPDATE qtss_setups
                  SET state        = 'closed_scratch',
                      close_reason = 'time_stop',
                      closed_at    = now(),
                      updated_at   = now(),
                      raw_meta     = raw_meta ||
                                     jsonb_build_object('time_stop_age_bars', $2::int)
                WHERE id = $1 AND closed_at IS NULL"#,
        )
        .bind(id)
        .bind(age_bars as i32)
        .execute(pool)
        .await;
        let _ = sqlx::query(
            r#"UPDATE live_positions
                  SET closed_at    = now(),
                      updated_at   = now(),
                      close_reason = 'time_stop',
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
        let _ = sqlx::query(
            r#"INSERT INTO notify_outbox
                  (title, body, channels, severity, event_key,
                   exchange, segment, symbol, status, dedup_key)
               VALUES ($1, $2, '["telegram"]'::jsonb, 'info',
                       'allocator_v2_time_stop',
                       'binance', 'futures', $3, 'pending', $4)
               ON CONFLICT (dedup_key) WHERE dedup_key IS NOT NULL DO NOTHING"#,
        )
        .bind(format!(
            "⌛ {symbol} · {timeframe} — time stop (age {age_bars}b ≥ cap {cap}b)"
        ))
        .bind(render_time_stop_html(&symbol, &timeframe, &profile, age_bars, cap))
        .bind(&symbol)
        .bind(format!("time_stop:{id}"))
        .execute(pool)
        .await;
        closed += 1;
    }
    Ok(closed)
}

fn render_time_stop_html(
    symbol: &str,
    timeframe: &str,
    profile: &str,
    age_bars: i64,
    cap: i64,
) -> String {
    format!(
        "<b>⌛ İŞLEM SONUCU — TIME STOP</b>\n\
         <b>📊 {symbol}</b> · <code>{timeframe}</code> · <i>profile {profile}</i>\n\
         <i>Sebep:</i> {age_bars} bar açıkta kaldı, TP1 vurulmadı (cap {cap} bar).\n\
         <i>Edge zamanla erir — sermaye daha iyi bir setup'a kaydırılıyor.</i>"
    )
}
