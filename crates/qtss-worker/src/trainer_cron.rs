//! Retraining cron loop — playbook FAZ_9B_RETRAINING_PLAYBOOK §2.
//!
//! Every `ai.retraining.cron_interval_secs` the loop wakes up and decides
//! whether to spawn the trainer, with a **typed trigger source** passed as
//! `QTSS_TRAINER_TRIGGER` so `qtss_ml_training_runs.trigger_source` gets
//! the right label. Decision order matches the playbook:
//!
//!   T6 bootstrap      — no active model + enough closed setups
//!   T4 outcome milestone — `n_new_closed ≥ trigger_min_new_closed`
//!   T2 cron / age     — active model older than `trigger_age_hours`
//!   else              — skip tick
//!
//! T1 (new-data) collapses into T4 since both fire on the same count; we
//! expose it via the `trigger_source` param when callers want to hand-pick.
//! T3 (drift) and T5 (manual) are triggered elsewhere — not by this loop.
//!
//! Subprocess rather than an in-process scheduler because the trainer is
//! Python. We bridge `DATABASE_URL → QTSS_DATABASE_URL` so the trainer
//! subprocess finds the DB.
//!
//! CLAUDE.md:
//!   #1 dispatch table for trigger decision (no nested if/else)
//!   #2 every threshold resolved from `system_config.ai.retraining.*`

use std::time::Duration;

use qtss_storage::{
    resolve_system_string, resolve_system_u64, resolve_worker_enabled_flag,
};
use sqlx::PgPool;
use tokio::process::Command;
use tracing::{debug, info, warn};

const MODULE: &str = "ai";

// Retraining config keys (migration 0169 §C).
const CFG_ENABLED: &str = "trainer.enabled";
const CFG_INTERVAL_SECS: &str = "retraining.cron_interval_secs";
const CFG_TRIGGER_MIN_NEW: &str = "retraining.trigger_min_new_closed";
const CFG_TRIGGER_AGE_H: &str = "retraining.trigger_age_hours";
const CFG_BINARY_PATH: &str = "retraining.binary_path";
const CFG_MIN_ROWS: &str = "trainer.min_rows";
const CFG_SIDECAR_URL: &str = "inference.sidecar_url";

// Env overrides for bootstrap / opt-out.
const ENV_ENABLED: &str = "QTSS_TRAINER_CRON_ENABLED";
const ENV_INTERVAL: &str = "QTSS_TRAINER_CRON_INTERVAL_SECS";
const ENV_BINARY: &str = "QTSS_TRAINER_BIN";
const ENV_SIDECAR_URL: &str = "QTSS_AI_INFERENCE_SIDECAR_URL";

const DEFAULT_INTERVAL_SECS: u64 = 3_600; // 1h (playbook migration default)
const DEFAULT_TRIGGER_MIN_NEW: u64 = 50;
const DEFAULT_TRIGGER_AGE_H: u64 = 168; // 7 days
const DEFAULT_MIN_ROWS: u64 = 500;
const DEFAULT_BINARY: &str = "qtss-trainer";
const DEFAULT_SIDECAR_URL: &str = "http://127.0.0.1:8790";

/// Trigger kind decided by the loop — surfaces as `QTSS_TRAINER_TRIGGER`
/// env var so the trainer audits `qtss_ml_training_runs.trigger_source`
/// with the same label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    Bootstrap,         // T6 — no active model
    OutcomeMilestone,  // T4 — n_new_closed ≥ threshold
    Cron,              // T2 — age threshold
    Skip,              // no reason to train this tick
}

impl Trigger {
    fn audit_label(self) -> &'static str {
        match self {
            Trigger::Bootstrap => "backfill",
            Trigger::OutcomeMilestone => "outcome_milestone",
            Trigger::Cron => "cron",
            Trigger::Skip => "manual", // never used; Skip exits early
        }
    }
}

pub async fn trainer_cron_loop(pool: PgPool) {
    info!("trainer cron: starting (playbook §2 decision flow)");
    probe_sidecar(&pool).await;

    loop {
        let enabled = resolve_worker_enabled_flag(&pool, MODULE, CFG_ENABLED, ENV_ENABLED, true).await;
        let interval_secs = resolve_system_u64(
            &pool, MODULE, CFG_INTERVAL_SECS, ENV_INTERVAL,
            DEFAULT_INTERVAL_SECS, 60, 24 * 3600,
        )
        .await;

        if enabled {
            match decide_trigger(&pool).await {
                Trigger::Skip => debug!("trainer cron: no trigger condition met, skipping tick"),
                t => {
                    info!(trigger = ?t, "trainer cron: firing trainer");
                    run_trainer(&pool, t).await;
                    probe_sidecar(&pool).await;
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

/// Playbook §2 decision table. Ordered by priority: bootstrap → outcome
/// milestone → age-based cron. First match wins.
async fn decide_trigger(pool: &PgPool) -> Trigger {
    let min_rows = resolve_system_u64(
        pool, MODULE, CFG_MIN_ROWS, "",
        DEFAULT_MIN_ROWS, 10, 1_000_000,
    )
    .await;
    let trigger_min_new = resolve_system_u64(
        pool, MODULE, CFG_TRIGGER_MIN_NEW, "",
        DEFAULT_TRIGGER_MIN_NEW, 1, 1_000_000,
    )
    .await;
    let trigger_age_h = resolve_system_u64(
        pool, MODULE, CFG_TRIGGER_AGE_H, "",
        DEFAULT_TRIGGER_AGE_H, 1, 24 * 365,
    )
    .await;

    let active = fetch_active_model(pool).await;
    let closed_total = count_closed_setups(pool, None).await;

    // T6: no active model but enough closed data → bootstrap.
    let Some(active) = active else {
        return if closed_total >= min_rows as i64 {
            Trigger::Bootstrap
        } else {
            debug!(closed_total, min_rows, "trainer cron: no active model + not enough rows");
            Trigger::Skip
        };
    };

    // T4: how many closed setups landed since the active model was trained?
    let n_new = count_closed_setups(pool, Some(active.trained_at_epoch_s)).await;
    if n_new >= trigger_min_new as i64 {
        return Trigger::OutcomeMilestone;
    }

    // T2: age-based.
    let age_h = active.age_hours();
    if age_h >= trigger_age_h as i64 {
        return Trigger::Cron;
    }

    debug!(n_new, trigger_min_new, age_h, trigger_age_h, "trainer cron: all thresholds below, skip");
    Trigger::Skip
}

struct ActiveModel {
    trained_at_epoch_s: i64,
}

impl ActiveModel {
    fn age_hours(&self) -> i64 {
        let now = chrono::Utc::now().timestamp();
        ((now - self.trained_at_epoch_s).max(0)) / 3600
    }
}

async fn fetch_active_model(pool: &PgPool) -> Option<ActiveModel> {
    let row = sqlx::query_as::<_, (chrono::DateTime<chrono::Utc>,)>(
        "SELECT trained_at FROM qtss_models WHERE active = true ORDER BY trained_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    Some(ActiveModel {
        trained_at_epoch_s: row.0.timestamp(),
    })
}

async fn count_closed_setups(pool: &PgPool, since_epoch_s: Option<i64>) -> i64 {
    // v_qtss_training_set_closed is the same view the trainer loads from,
    // so this count is the exact "rows trainer would see" number.
    let q = match since_epoch_s {
        Some(_) => "SELECT COUNT(*) FROM v_qtss_training_set_closed WHERE closed_at > to_timestamp($1)",
        None    => "SELECT COUNT(*) FROM v_qtss_training_set_closed",
    };
    let mut query = sqlx::query_as::<_, (i64,)>(q);
    if let Some(s) = since_epoch_s {
        query = query.bind(s);
    }
    query
        .fetch_one(pool)
        .await
        .map(|(n,)| n)
        .unwrap_or_else(|e| {
            warn!(%e, "trainer cron: closed-setup count query failed");
            0
        })
}

async fn run_trainer(pool: &PgPool, trigger: Trigger) {
    let bin = resolve_system_string(
        pool, MODULE, CFG_BINARY_PATH, ENV_BINARY, DEFAULT_BINARY,
    )
    .await;

    let trigger_label = trigger.audit_label();
    info!(binary = %bin, trigger = trigger_label, "trainer cron: invoking");

    // The configured binary path may be a bare command ("qtss-trainer")
    // or an absolute path ("/app/qtss/trainer/.venv/bin/qtss-trainer").
    // Argument tokens like `--notes "auto"` are passed as-is for operator
    // flexibility (escape-free because split_whitespace is sufficient here).
    let parts: Vec<&str> = bin.split_whitespace().collect();
    let Some((head, tail)) = parts.split_first() else {
        warn!("trainer cron: empty binary path");
        return;
    };

    let mut command = Command::new(head);
    command.args(tail);
    command.arg("train");
    command.arg("--trigger-source").arg(trigger_label);

    // Trainer reads QTSS_DATABASE_URL; worker uses DATABASE_URL. Bridge.
    if std::env::var_os("QTSS_DATABASE_URL").is_none() {
        if let Ok(url) = std::env::var("DATABASE_URL") {
            command.env("QTSS_DATABASE_URL", url);
        }
    }
    // Belt + suspenders: trainer also falls back to env for trigger.
    command.env("QTSS_TRAINER_TRIGGER", trigger_label);

    match command.output().await {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            info!(stdout = %stdout.trim(), "trainer cron: success");
        }
        Ok(o) => log_trainer_nonzero(&o),
        Err(e) => warn!(error = %e, "trainer cron: failed to spawn"),
    }
}

/// Exit-code → log-level dispatch (CLAUDE.md #1).
fn log_trainer_nonzero(o: &std::process::Output) {
    let code = o.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
    // Expected "not enough signal" codes are audited on the Python side
    // into qtss_ml_training_runs; we downgrade the Rust log to info.
    let soft = matches!(code, 2 | 3 | 4 | 5);
    if soft {
        info!(code, stderr, "trainer cron: skipped (gate failed — see qtss_ml_training_runs)");
    } else {
        warn!(code, stderr, "trainer cron: trainer exited non-zero");
    }
}

async fn probe_sidecar(pool: &PgPool) {
    let url = resolve_system_string(
        pool, MODULE, CFG_SIDECAR_URL, ENV_SIDECAR_URL, DEFAULT_SIDECAR_URL,
    )
    .await;
    let probe = format!("{}/health", url.trim_end_matches('/'));
    let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(2)).build() else {
        return;
    };
    match client.get(&probe).send().await {
        Ok(r) if r.status().is_success() => info!(url = %probe, "ai sidecar: up"),
        Ok(r) => warn!(url = %probe, status = %r.status(), "ai sidecar: unhealthy"),
        Err(e) => warn!(url = %probe, error = %e, "ai sidecar: unreachable"),
    }
}
