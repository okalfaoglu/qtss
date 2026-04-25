// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `weight_tuner_loop` — post-trade learning (v1.2.3).
//!
//! ChatGPT teardown alpha #5: every trade leaves a footprint in
//! `qtss_setups` (primary_family, realized_pnl_pct, close_reason).
//! Aggregating those by family + profile gives a winrate / EV per
//! family. Allocator's confluence weights should pick up the slack:
//! losing families → weight nudged DOWN, winning families → nudged UP.
//!
//! Tick: hourly by default. Read last `lookback_days` of closed
//! setups, compute (family, profile) → EV in pct. Push the new
//! weight back into `system_config.confluence.weights.{family}` —
//! the confluence scorer reloads weights every tick so changes
//! propagate automatically.
//!
//! Guardrails:
//!   * Sample-size floor (`min_sample`) below which weight is left
//!     unchanged — cold start has no signal.
//!   * Drift cap (`max_drift_pct`) per tick so an outlier week
//!     doesn't kill a strategy.
//!   * Floor + ceiling on absolute weight to keep family from being
//!     zeroed out or trampling the rest.

use std::time::Duration;

use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

pub async fn weight_tuner_loop(pool: PgPool) {
    info!("weight_tuner_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            continue;
        }
        match run_tick(&pool).await {
            Ok(n) => info!(updated = n, "weight_tuner tick ok"),
            Err(e) => warn!(%e, "weight_tuner tick failed"),
        }
        let secs = load_tick_secs(&pool).await;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='weight_tuner' AND config_key='enabled'",
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
           WHERE module='weight_tuner' AND config_key='tick_secs'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 3600; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("secs").and_then(|v| v.as_u64()).unwrap_or(3600).max(60)
}

async fn load_param(pool: &PgPool, key: &str, default: f64) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module='weight_tuner' AND config_key=$1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return default; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    match &val {
        Value::Number(n) => n.as_f64().unwrap_or(default),
        other => other.get("value").and_then(|v| v.as_f64()).unwrap_or(default),
    }
}

async fn run_tick(pool: &PgPool) -> anyhow::Result<usize> {
    let lookback_days = load_param(pool, "lookback_days", 14.0).await as i32;
    let min_sample = load_param(pool, "min_sample", 10.0).await as i64;
    let max_drift_pct = load_param(pool, "max_drift_pct", 0.10).await; // 10% per tick
    let weight_floor = load_param(pool, "weight_floor", 0.1).await;
    let weight_ceiling = load_param(pool, "weight_ceiling", 2.0).await;

    // Per-family stats. We aggregate across all profiles + symbols
    // for now — the regime-aware adjuster matrix in confluence is the
    // place to slice further once FAZ 24 lands.
    let rows = sqlx::query(
        r#"SELECT raw_meta->>'primary_family' AS family,
                  COUNT(*)::bigint AS n,
                  COUNT(*) FILTER (WHERE realized_pnl_pct > 0)::bigint AS wins,
                  COALESCE(AVG(CASE WHEN realized_pnl_pct > 0 THEN realized_pnl_pct END), 0)::float8
                    AS avg_win_pct,
                  COALESCE(AVG(CASE WHEN realized_pnl_pct < 0 THEN abs(realized_pnl_pct) END), 0)::float8
                    AS avg_loss_pct
             FROM qtss_setups
            WHERE closed_at IS NOT NULL
              AND closed_at >= now() - make_interval(days => $1::int)
              AND raw_meta ? 'primary_family'
              AND raw_meta->>'primary_family' IS NOT NULL
              AND realized_pnl_pct IS NOT NULL
              AND close_reason IN ('tp_final','sl_hit','trail_stop','invalidated')
            GROUP BY family"#,
    )
    .bind(lookback_days)
    .fetch_all(pool)
    .await?;

    let mut updated = 0usize;
    for r in rows {
        let family: Option<String> = r.try_get("family").ok();
        let Some(family) = family else { continue };
        let n: i64 = r.try_get("n").unwrap_or(0);
        if n < min_sample {
            debug!(%family, n, "weight_tuner: below min_sample, skip");
            continue;
        }
        let wins: i64 = r.try_get("wins").unwrap_or(0);
        let winrate = wins as f64 / n as f64;
        let avg_win: f64 = r.try_get("avg_win_pct").unwrap_or(0.0);
        let avg_loss: f64 = r.try_get("avg_loss_pct").unwrap_or(0.0);
        let ev_pct = winrate * avg_win - (1.0 - winrate) * avg_loss;

        // Read current weight for this family.
        let current = read_weight(pool, &family).await;
        // EV-driven nudge. Map ev_pct ∈ [-2, +2] (typical R-scaled
        // realised PnL) to multiplier in [1 - drift, 1 + drift]
        // linearly. EV = 0 → no change; EV > 0 → up; EV < 0 → down.
        let nudge = (ev_pct / 2.0).clamp(-1.0, 1.0) * max_drift_pct;
        let proposed = (current * (1.0 + nudge))
            .clamp(weight_floor, weight_ceiling);

        if (proposed - current).abs() < 0.001 {
            debug!(%family, current, proposed, "weight_tuner: no-op (within precision)");
            continue;
        }

        info!(
            %family, n, winrate, avg_win, avg_loss, ev_pct,
            current, proposed, drift = proposed - current,
            "weight_tuner: nudging family weight"
        );
        write_weight(pool, &family, proposed).await;
        // Write a ledger row so we can audit what fired when.
        let _ = sqlx::query(
            r#"INSERT INTO weight_tuner_history
                  (family, samples, winrate, avg_win_pct, avg_loss_pct,
                   ev_pct, prev_weight, new_weight)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
        )
        .bind(&family)
        .bind(n)
        .bind(winrate)
        .bind(avg_win)
        .bind(avg_loss)
        .bind(ev_pct)
        .bind(current)
        .bind(proposed)
        .execute(pool)
        .await;
        updated += 1;
    }
    Ok(updated)
}

async fn read_weight(pool: &PgPool, family: &str) -> f64 {
    let key = format!("weights.{family}");
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module='confluence' AND config_key=$1",
    )
    .bind(&key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 1.0; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("value").and_then(|v| v.as_f64()).unwrap_or(1.0)
}

async fn write_weight(pool: &PgPool, family: &str, new_weight: f64) {
    let key = format!("weights.{family}");
    let value = json!({"value": new_weight});
    let _ = sqlx::query(
        r#"INSERT INTO system_config (module, config_key, value, description)
           VALUES ('confluence', $1, $2, $3)
           ON CONFLICT (module, config_key) DO UPDATE
              SET value = EXCLUDED.value,
                  updated_at = now()"#,
    )
    .bind(&key)
    .bind(&value)
    .bind(format!(
        "Auto-tuned by weight_tuner_loop (post-trade learning). \
         Last touched: {}",
        chrono::Utc::now().to_rfc3339()
    ))
    .execute(pool)
    .await;
}
