//! Faz 9.8.12 — periodic trainer invocation.
//!
//! Spawns `python -m qtss_trainer train` every N hours (default 168h
//! = weekly). The Python side loads closed+labeled setups, trains a
//! LightGBM model, and inserts a row into `qtss_models`. If there
//! aren't enough closed setups yet the trainer exits with code 2 and
//! we log and move on — no crash.
//!
//! This is deliberately a *subprocess* rather than a scheduler job
//! because the trainer crate is Python and already has its own DB
//! wiring. Running it from Rust via `tokio::process::Command` keeps
//! the call site simple and lets ops roll back by disabling one flag.
//!
//! Health log: at each tick we also log whether the AI inference
//! sidecar responds on `/health`, so a down sidecar shows up in the
//! worker log instead of silently producing empty AI Shadow feeds.

use std::time::Duration;

use qtss_storage::{
    resolve_system_string, resolve_system_u64, resolve_worker_enabled_flag,
};
use sqlx::PgPool;
use tokio::process::Command;
use tracing::{info, warn};

const MODULE: &str = "ai";
const CFG_ENABLED: &str = "trainer.cron.enabled";
const CFG_INTERVAL_H: &str = "trainer.cron.interval_hours";
const CFG_COMMAND: &str = "trainer.cron.command";
const CFG_SIDECAR_URL: &str = "inference.sidecar_url";

const ENV_ENABLED: &str = "QTSS_TRAINER_CRON_ENABLED";
const ENV_INTERVAL: &str = "QTSS_TRAINER_CRON_INTERVAL_HOURS";
const ENV_COMMAND: &str = "QTSS_TRAINER_CMD";
const ENV_SIDECAR_URL: &str = "QTSS_AI_INFERENCE_SIDECAR_URL";

const DEFAULT_INTERVAL_H: u64 = 168; // weekly
const DEFAULT_COMMAND: &str = "python -m qtss_trainer train";
const DEFAULT_SIDECAR_URL: &str = "http://127.0.0.1:8790";

pub async fn trainer_cron_loop(pool: PgPool) {
    info!("trainer cron: starting");
    // One eager sidecar health probe at startup so ops sees it immediately.
    probe_sidecar(&pool).await;

    loop {
        let enabled =
            resolve_worker_enabled_flag(&pool, MODULE, CFG_ENABLED, ENV_ENABLED, true).await;
        let interval_h = resolve_system_u64(
            &pool, MODULE, CFG_INTERVAL_H, ENV_INTERVAL,
            DEFAULT_INTERVAL_H, 1, 24 * 30,
        )
        .await;

        if enabled {
            run_trainer(&pool).await;
            probe_sidecar(&pool).await;
        }

        tokio::time::sleep(Duration::from_secs(interval_h * 3600)).await;
    }
}

async fn run_trainer(pool: &PgPool) {
    let cmd_line = resolve_system_string(
        pool, MODULE, CFG_COMMAND, ENV_COMMAND, DEFAULT_COMMAND,
    )
    .await;
    info!(command = %cmd_line, "trainer cron: invoking trainer");

    let parts: Vec<&str> = cmd_line.split_whitespace().collect();
    let Some((head, tail)) = parts.split_first() else {
        warn!("trainer cron: empty command");
        return;
    };

    // Trainer reads `QTSS_DATABASE_URL`; worker/.env only exports
    // `DATABASE_URL`. Bridge it here so the subprocess finds the DB
    // without requiring a duplicate entry in .env.
    let mut command = Command::new(head);
    command.args(tail);
    if std::env::var_os("QTSS_DATABASE_URL").is_none() {
        if let Ok(url) = std::env::var("DATABASE_URL") {
            command.env("QTSS_DATABASE_URL", url);
        }
    }
    let out = command.output().await;
    match out {
        Ok(o) if o.status.success() => {
            info!(
                stdout = %String::from_utf8_lossy(&o.stdout).trim(),
                "trainer cron: done"
            );
        }
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            // Exit 2 = not enough rows. Log at info, not warn — it's
            // expected while the outcome labeler is still accumulating.
            let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
            if code == 2 {
                info!(code, stderr, "trainer cron: insufficient rows — skipping");
            } else {
                warn!(code, stderr, "trainer cron: trainer exited non-zero");
            }
        }
        Err(e) => warn!(error = %e, "trainer cron: failed to spawn trainer"),
    }
}

async fn probe_sidecar(pool: &PgPool) {
    let url = resolve_system_string(
        pool, MODULE, CFG_SIDECAR_URL, ENV_SIDECAR_URL, DEFAULT_SIDECAR_URL,
    )
    .await;
    let probe = format!("{}/health", url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build();
    let Ok(client) = client else { return };
    match client.get(&probe).send().await {
        Ok(r) if r.status().is_success() => info!(url = %probe, "ai sidecar: up"),
        Ok(r) => warn!(url = %probe, status = %r.status(), "ai sidecar: unhealthy"),
        Err(e) => warn!(url = %probe, error = %e, "ai sidecar: unreachable"),
    }
}
