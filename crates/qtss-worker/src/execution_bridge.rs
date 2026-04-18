//! Faz 9.8.11 — execution bridge worker.
//!
//! Claims rows from `selected_candidates` (FOR UPDATE SKIP LOCKED) and
//! dispatches them.  This version lands a *minimal viable dispatch*:
//!
//! - Dry mode: insert a row into `exchange_orders` flagged as a paper
//!   fill (venue_order_id = NULL, status = 'filled'), so the dry GUI
//!   and downstream analytics (PnL, Training Set, AI Shadow) see real
//!   rows for every selected candidate.
//! - Live mode: gated off by default (`execution.live.enabled=false`).
//!   When flipped on a future patch will wire the broker gateway; for
//!   now live rows are marked `errored` with a clear message so they
//!   don't sit in `pending` indefinitely.
//!
//! Keeping the bridge thin on purpose: the heavy lifting (order sizing,
//! slippage guard, liquidation guard) already ran upstream in the
//! allocator and risk engine. The bridge's job is to *close the loop*
//! so the GUI pages (Training Set, Model Registry, AI Shadow, live
//! positions) stop looking like abandoned skeletons.

use std::time::Duration;

use qtss_storage::{
    claim_selected_candidates, mark_selected_errored, mark_selected_placed,
    resolve_system_u64, resolve_worker_enabled_flag, SelectedCandidateRow,
};
use sqlx::PgPool;
use tracing::{debug, info, warn};

const MODULE: &str = "execution";
const CFG_INTERVAL_MS: &str = "execution.loop_interval_ms";
const CFG_DRY_ENABLED: &str = "execution.dry.enabled";
const CFG_LIVE_ENABLED: &str = "execution.live.enabled";
const ENV_INTERVAL: &str = "QTSS_EXEC_BRIDGE_INTERVAL_MS";
const ENV_DRY: &str = "QTSS_EXEC_DRY_ENABLED";
const ENV_LIVE: &str = "QTSS_EXEC_LIVE_ENABLED";

const DEFAULT_INTERVAL_MS: u64 = 2_000;
const BATCH: i64 = 10;

pub async fn execution_bridge_loop(pool: PgPool) {
    info!("execution bridge worker: starting");
    loop {
        let interval_ms = resolve_system_u64(
            &pool, MODULE, CFG_INTERVAL_MS, ENV_INTERVAL,
            DEFAULT_INTERVAL_MS, 500, 600_000,
        )
        .await;
        if let Err(e) = run_tick(&pool).await {
            warn!(error=%e, "execution bridge tick failed");
        }
        tokio::time::sleep(Duration::from_millis(interval_ms.max(500))).await;
    }
}

async fn run_tick(pool: &PgPool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dry_enabled =
        resolve_worker_enabled_flag(pool, MODULE, CFG_DRY_ENABLED, ENV_DRY, true).await;
    let live_enabled =
        resolve_worker_enabled_flag(pool, MODULE, CFG_LIVE_ENABLED, ENV_LIVE, false).await;
    let rows = claim_selected_candidates(pool, BATCH).await?;
    if rows.is_empty() {
        return Ok(());
    }
    for row in rows {
        let outcome = dispatch(pool, &row, dry_enabled, live_enabled).await;
        match outcome {
            Ok(()) => mark_selected_placed(pool, row.id).await?,
            Err(e) => {
                let msg = e.to_string();
                warn!(id = row.id, setup = %row.setup_id, error = %msg, "candidate dispatch failed");
                mark_selected_errored(pool, row.id, &msg).await?;
            }
        }
    }
    Ok(())
}

async fn dispatch(
    pool: &PgPool,
    row: &SelectedCandidateRow,
    dry_enabled: bool,
    live_enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match row.mode.as_str() {
        "dry" if dry_enabled => dispatch_dry(pool, row).await,
        "dry" => Err("dry execution disabled via config".into()),
        "live" if live_enabled => Err("live execution adapter not yet wired (Faz 9.8.12)".into()),
        "live" => Err("live execution disabled via config".into()),
        "backtest" => Ok(()), // backtest rows are consumed by the backtest runner, not the bridge
        other => Err(format!("unknown mode: {other}").into()),
    }
}

async fn dispatch_dry(
    pool: &PgPool,
    row: &SelectedCandidateRow,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Mirror the intent as a paper fill so the dry GUI + Training Set
    // pipeline see real traffic. Schema-wise we match the DryOrdersMirror
    // contract already used by strategy_runner: no venue_order_id,
    // status='filled', filled_qty = qty, strategy_key tagged.
    let qty = rust_decimal::Decimal::new(1, 2); // placeholder 0.01 — sizing lives upstream
    sqlx::query(
        r#"
        INSERT INTO exchange_orders (
            exchange, symbol, side, order_type, qty, price,
            status, filled_qty, avg_price, strategy_key, dry_run,
            created_at, updated_at
        )
        VALUES ($1,$2,$3,'market',$4,$5,'filled',$4,$5,'selector',TRUE,
                now(),now())
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(&row.exchange)
    .bind(&row.symbol)
    .bind(side_str(&row.direction))
    .bind(qty)
    .bind(row.entry_price)
    .execute(pool)
    .await?;
    debug!(setup = %row.setup_id, "dry dispatch ok");
    Ok(())
}

fn side_str(d: &str) -> &'static str {
    match d {
        "long" => "buy",
        _ => "sell",
    }
}
