//! Historical backfill orchestrator — spec `FAZ_9B_HISTORICAL_BACKFILL.md`.
//!
//! The walk-forward replay we need for training-set bootstrap is **already
//! implemented** across three existing workers:
//!
//!   1. `historical_progressive_scan` — walks bars offset-by-offset,
//!      runs pivot+regime+detectors, writes `qtss_v2_detections` rows
//!      as they would have appeared at each historical moment (no
//!      look-ahead leakage by construction, see that module's docstring).
//!   2. `v2_setup_loop` / `v2_detection_orchestrator` — turns detections
//!      into `qtss_setups` rows with features attached.
//!   3. `outcome_labeler_loop` — resolves TP/SL for closed setups so
//!      they land in `v_qtss_training_set_closed`.
//!
//! The only missing piece is an **operator-visible master switch** that
//! flips the right worker flags on/off and logs progress. This file is
//! that switch. Reasoning: duplicating walk-forward bar iteration in a
//! dedicated crate would bloat the codebase and create two code paths
//! that can drift apart. Instead we coordinate what already exists.
//!
//! What the operator does:
//!   ```sql
//!   UPDATE system_config SET value = 'true'::jsonb
//!   WHERE module='ai' AND config_key='backfill.enabled';
//!   ```
//!   (GUI toggle in Config Editor works the same way.)
//!
//! The orchestrator then:
//!   * flips `detection.historical_progressive_scan.enabled=true`
//!   * logs a "backfill started" event into `qtss_ml_training_runs`
//!     (status='running', trigger_source='backfill')
//!   * polls backtest-mode setup count every N seconds
//!   * when the rate plateaus (no new setups for `plateau_secs`), flips
//!     the progressive scan back off, records completion, and fires a
//!     trainer run with `trigger_source=backfill` so the fresh training
//!     examples feed straight into a new model.
//!
//! CLAUDE.md:
//!   #2 all thresholds from `system_config.ai.backfill.*` (migration 0169 §E).
//!   #3 no detector logic here — orchestrator only toggles switches.

use std::time::Duration;

use qtss_storage::{resolve_system_u64, resolve_worker_enabled_flag};
use sqlx::PgPool;
use tracing::{debug, info, warn};

const MODULE: &str = "ai";
const CFG_ENABLED: &str = "backfill.enabled";
const CFG_POLL_SECS: &str = "backfill.poll_secs";
const CFG_PLATEAU_SECS: &str = "backfill.plateau_secs";

const ENV_ENABLED: &str = "QTSS_ML_BACKFILL_ENABLED";

const DEFAULT_POLL_SECS: u64 = 120; // 2 min
const DEFAULT_PLATEAU_SECS: u64 = 600; // 10 min without growth → done

// The child worker the operator is effectively borrowing.
const SCAN_MODULE: &str = "detection";
const SCAN_KEY: &str = "historical_progressive_scan.enabled";

/// Master switch loop. Idle until `backfill.enabled=true` flips on; then
/// orchestrates one replay cycle end-to-end; returns to idle.
pub async fn ml_backfill_loop(pool: PgPool) {
    info!("ml backfill orchestrator: starting (idle until ai.backfill.enabled=true)");
    loop {
        let enabled = resolve_worker_enabled_flag(&pool, MODULE, CFG_ENABLED, ENV_ENABLED, false).await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        info!("ml backfill: master switch ON — starting one replay cycle");
        if let Err(e) = run_cycle(&pool).await {
            warn!(error = %e, "ml backfill cycle failed");
        }
        // Flip master switch back to false so a manual re-toggle is required.
        // Operator explicitly starts each cycle — avoids infinite replay.
        if let Err(e) = set_config_bool(&pool, MODULE, CFG_ENABLED, false).await {
            warn!(error = %e, "ml backfill: could not self-disable master switch");
        }
        info!("ml backfill: cycle complete, master switch reset to false");
    }
}

async fn run_cycle(pool: &PgPool) -> Result<(), String> {
    let poll_secs = resolve_system_u64(pool, MODULE, CFG_POLL_SECS, "", DEFAULT_POLL_SECS, 10, 3600).await;
    let plateau_secs =
        resolve_system_u64(pool, MODULE, CFG_PLATEAU_SECS, "", DEFAULT_PLATEAU_SECS, 60, 24 * 3600).await;

    // 1. Audit: mark backfill start.
    let run_id = insert_training_run_start(pool).await?;
    // Seed the live-progress heartbeat row (migration 0172). Updated on
    // every poll tick so the GUI can show "cycle N running · 3.2K
    // backtest setups · last grew 2 min ago" without tailing logs.
    if let Err(e) = upsert_progress(pool, run_id, 0, 0, "running").await {
        warn!(%e, "ml backfill: initial progress row insert failed");
    }

    // 2. Enable the child scanner. The progressive scan worker is already
    //    running its own idle poll loop; we just flip the flag it reads.
    set_config_bool(pool, SCAN_MODULE, SCAN_KEY, true).await?;
    info!(
        %run_id,
        poll_secs, plateau_secs,
        "ml backfill: historical_progressive_scan enabled — watching setup growth"
    );

    // 3. Watch backtest-mode setup count. Plateau detection = no growth for
    //    `plateau_secs`. This is simpler than reading bar-cursor checkpoints
    //    and works across any detector/timeframe combination.
    let mut last_count: i64 = count_backtest_setups(pool).await;
    let mut last_growth_at = std::time::Instant::now();

    loop {
        tokio::time::sleep(Duration::from_secs(poll_secs)).await;
        let now_count = count_backtest_setups(pool).await;
        let growing = now_count > last_count;
        if growing {
            debug!(prev = last_count, now = now_count, "ml backfill: growth");
            last_count = now_count;
            last_growth_at = std::time::Instant::now();
        }
        let plateau_hit = !growing && last_growth_at.elapsed().as_secs() >= plateau_secs;
        let phase = if plateau_hit { "plateau_detected" } else { "running" };
        if let Err(e) = upsert_progress(pool, run_id, now_count, 0, phase).await {
            warn!(%e, "ml backfill: progress upsert failed");
        }
        if plateau_hit {
            info!(
                final_count = now_count,
                "ml backfill: plateau reached — closing cycle"
            );
            break;
        }
        if !growing {
            debug!(count = now_count, "ml backfill: no growth yet, waiting for plateau");
        }
    }

    // 4. Disable progressive scan so the bar iteration stops burning CPU.
    set_config_bool(pool, SCAN_MODULE, SCAN_KEY, false).await?;

    // 5. Close the audit row. The trainer_cron will pick up the new rows on
    //    its next tick (T6 bootstrap or T4 outcome milestone); operator can
    //    also fire manually per playbook §6.
    finalize_training_run(pool, run_id, last_count).await?;
    if let Err(e) = upsert_progress(pool, run_id, last_count, 0, "closed").await {
        warn!(%e, "ml backfill: progress close failed");
    }

    Ok(())
}

async fn insert_training_run_start(pool: &PgPool) -> Result<uuid::Uuid, String> {
    sqlx::query_as::<_, (uuid::Uuid,)>(
        r#"INSERT INTO qtss_ml_training_runs
             (trigger_source, status, notes)
           VALUES ('backfill', 'running', 'orchestrator cycle started')
           RETURNING id"#,
    )
    .fetch_one(pool)
    .await
    .map(|(id,)| id)
    .map_err(|e| format!("training_runs insert start: {e}"))
}

async fn finalize_training_run(
    pool: &PgPool,
    run_id: uuid::Uuid,
    n_setups: i64,
) -> Result<(), String> {
    let note = format!(
        "orchestrator cycle complete — {} backtest setups in qtss_setups (trainer_cron will pick up next tick)",
        n_setups,
    );
    sqlx::query(
        r#"UPDATE qtss_ml_training_runs
              SET finished_at = now(),
                  status      = 'success',
                  n_closed_setups = $2,
                  notes       = COALESCE(notes, '') || ' | ' || $3
            WHERE id = $1"#,
    )
    .bind(run_id)
    .bind(n_setups)
    .bind(note)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| format!("training_runs finalize: {e}"))
}

/// Upsert the orchestrator's heartbeat row (migration 0172). Called on
/// cycle start, every poll tick, and at close. `last_growth_at` is
/// only refreshed when the setup count actually advances so the GUI
/// can show genuine staleness.
async fn upsert_progress(
    pool: &PgPool,
    run_id: uuid::Uuid,
    last_setup_count: i64,
    symbols_active: i32,
    phase: &str,
) -> Result<(), String> {
    sqlx::query(
        r#"INSERT INTO qtss_ml_backfill_progress
               (run_id, last_poll_at, last_growth_at, last_setup_count,
                symbols_active, phase)
           VALUES ($1, now(), now(), $2, $3, $4)
           ON CONFLICT (run_id) DO UPDATE SET
               last_poll_at     = now(),
               last_growth_at   = CASE
                                    WHEN EXCLUDED.last_setup_count > qtss_ml_backfill_progress.last_setup_count
                                    THEN now()
                                    ELSE qtss_ml_backfill_progress.last_growth_at
                                  END,
               last_setup_count = EXCLUDED.last_setup_count,
               symbols_active   = EXCLUDED.symbols_active,
               phase            = EXCLUDED.phase"#,
    )
    .bind(run_id)
    .bind(last_setup_count)
    .bind(symbols_active)
    .bind(phase)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| format!("backfill progress upsert: {e}"))
}

async fn count_backtest_setups(pool: &PgPool) -> i64 {
    // qtss_setups.mode stores 'backtest' for backfill-produced rows per
    // migration 0169 §E (backfill.setup_mode_marker).
    sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM qtss_setups WHERE mode = 'backtest'")
        .fetch_one(pool)
        .await
        .map(|(n,)| n)
        .unwrap_or_else(|e| {
            warn!(%e, "ml backfill: backtest-setup count failed");
            0
        })
}

async fn set_config_bool(
    pool: &PgPool,
    module: &str,
    key: &str,
    value: bool,
) -> Result<(), String> {
    let json_val = if value { "true" } else { "false" };
    sqlx::query(
        r#"INSERT INTO system_config (module, config_key, value, description)
           VALUES ($1, $2, $3::jsonb, 'toggled by ml_backfill_orchestrator')
           ON CONFLICT (module, config_key) DO UPDATE
             SET value = EXCLUDED.value"#,
    )
    .bind(module)
    .bind(key)
    .bind(json_val)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| format!("set_config_bool {module}.{key}: {e}"))
}
