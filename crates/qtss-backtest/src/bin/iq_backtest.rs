//! `iq-backtest` CLI — single-config backtest runner.
//!
//! Usage:
//!   iq-backtest --config path/to/config.json [--log path] [--json-logs]
//!
//! When --log is a directory, the CLI appends a stamped JSONL
//! filename. The trade log carries one (trade, attribution) row per
//! line — feed it into pandas / DuckDB / jq for analysis.
//!
//! Exit codes:
//!   0 — backtest ran, report printed.
//!   1 — config / DB / IO error.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;

use qtss_backtest::iq::cli::{
    init_tracing, init_tracing_json, load_config_file, print_report,
    resolve_log_path,
};
use qtss_backtest::iq::persistence;
use qtss_backtest::iq::{
    CostModel, IqBacktestRunner, IqLifecycleManager, IqReplayDetector,
};

fn main() -> Result<()> {
    // Naive arg parse — no clap dep yet to keep deps light. The
    // backtest runner is the heavy machinery; CLI is glue.
    let args: Vec<String> = std::env::args().collect();
    let mut config_path: Option<PathBuf> = None;
    let mut log_path: Option<PathBuf> = None;
    let mut json_logs = false;
    let mut persist = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--config" | "-c" => {
                config_path = Some(PathBuf::from(
                    args.get(i + 1).cloned().unwrap_or_default(),
                ));
                i += 2;
            }
            "--log" | "-l" => {
                log_path = Some(PathBuf::from(
                    args.get(i + 1).cloned().unwrap_or_default(),
                ));
                i += 2;
            }
            "--json-logs" => {
                json_logs = true;
                i += 1;
            }
            "--persist" => {
                persist = true;
                i += 1;
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            _ => {
                eprintln!("unknown arg: {}", args[i]);
                print_usage();
                std::process::exit(1);
            }
        }
    }

    if json_logs {
        init_tracing_json();
    } else {
        init_tracing();
    }

    let Some(config_path) = config_path else {
        eprintln!("--config is required");
        print_usage();
        std::process::exit(1);
    };

    let mut config = load_config_file(&config_path)
        .with_context(|| format!("loading config from {}", config_path.display()))?;
    if let Some(p) = &log_path {
        config.trade_log_path = Some(resolve_log_path(p, &config.run_tag));
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;
    let trade_log_path_for_persist = config
        .trade_log_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let report = rt.block_on(async move {
        let database_url = std::env::var("DATABASE_URL")
            .or_else(|_| std::env::var("QTSS_DATABASE_URL"))
            .context("DATABASE_URL or QTSS_DATABASE_URL env var must be set")?;
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(&database_url)
            .await
            .context("connecting to Postgres")?;
        // BUG6 fix — wire the real detector + lifecycle. Default
        // `IqBacktestRunner::new()` was leaving NoSetups + NoLifecycle
        // installed, so the bar loop ran in 17ms with 0 trades. The
        // actual replay logic lives in IqReplayDetector +
        // IqLifecycleManager and needs to be plugged in here.
        let detector = Arc::new(IqReplayDetector::new(config.clone()));
        let lifecycle = Arc::new(IqLifecycleManager::new(
            config.clone(),
            CostModel::default(),
        ));
        let runner = IqBacktestRunner::new(config)?
            .with_detector(detector)
            .with_lifecycle(lifecycle);
        let report = runner.run(&pool).await?;
        if persist {
            match persistence::persist_report(
                &pool,
                &report,
                trade_log_path_for_persist.as_deref(),
            )
            .await
            {
                Ok(id) => {
                    println!();
                    println!(" persisted run id: {}", id);
                    println!(" view: /v2/iq-backtest/runs/{}", id);
                }
                Err(e) => {
                    eprintln!("WARN: persist failed: {e}");
                }
            }
        }
        Ok::<_, anyhow::Error>(report)
    })?;

    print_report(&report);
    Ok(())
}

fn print_usage() {
    eprintln!(
        "usage: iq-backtest --config <path> [--log <path>] [--persist] [--json-logs]\n\
         \n\
         args:\n\
         \x20 --config, -c   Path to JSON config (IqBacktestConfig).\n\
         \x20 --log, -l      Output JSONL trade log (file or directory).\n\
         \x20 --persist      Save aggregate report to iq_backtest_runs table.\n\
         \x20 --json-logs    Emit tracing events as JSON lines.\n\
         \x20 --help, -h     Show this message.\n\
         \n\
         env:\n\
         \x20 DATABASE_URL or QTSS_DATABASE_URL — Postgres connection string."
    );
}
