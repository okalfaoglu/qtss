//! Periodic drawdown snapshot — persists (peak_equity, equity, drawdown_pct)
//! to `account_drawdown_snapshots` so the data survives restarts and feeds
//! the history chart.
//!
//! Reads equity from the same bootstrap path as the risk bridge
//! (`risk.bootstrap.equity`), and peak from the last persisted snapshot
//! (updated in-memory when equity exceeds it). On fresh start, seeds peak
//! from the latest row in the table (or equity if none exists).

use std::time::Duration;

use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{info, warn};

use qtss_storage::{
    resolve_system_f64, resolve_worker_tick_secs, AccountDrawdownRepository,
};

pub async fn v2_drawdown_snapshot_loop(pool: PgPool) {
    info!("v2_drawdown_snapshot_loop: started");
    let repo = AccountDrawdownRepository::new(pool.clone());

    // We track a single operator account; user_id comes from config.
    let user_id_str = qtss_storage::resolve_system_string(
        &pool,
        "risk",
        "bootstrap.user_id",
        "QTSS_RISK_BOOTSTRAP_USER_ID",
        "",
    )
    .await;
    let user_id = match uuid::Uuid::parse_str(user_id_str.trim()) {
        Ok(u) => u,
        Err(_) => {
            warn!("v2_drawdown_snapshot: risk.bootstrap.user_id not set, sleeping forever");
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        }
    };

    let exchange = qtss_storage::resolve_system_string(
        &pool,
        "risk",
        "bootstrap.exchange",
        "QTSS_RISK_BOOTSTRAP_EXCHANGE",
        "binance",
    )
    .await;

    // Seed peak from last persisted snapshot.
    let mut peak = match repo.latest(user_id, &exchange).await {
        Ok(Some(row)) => row.peak_equity,
        _ => Decimal::ZERO,
    };

    loop {
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "drawdown_snapshot_tick_secs",
            "QTSS_DRAWDOWN_SNAPSHOT_TICK_SECS",
            300,
            60,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(tick)).await;

        let equity_f = resolve_system_f64(
            &pool,
            "risk",
            "bootstrap.equity",
            "QTSS_RISK_BOOTSTRAP_EQUITY",
            10_000.0,
        )
        .await;
        let equity = Decimal::from_f64(equity_f).unwrap_or_else(|| Decimal::from(10_000));

        if equity > peak {
            peak = equity;
        }
        if peak == Decimal::ZERO {
            peak = equity;
        }

        let dd_pct = if peak > Decimal::ZERO {
            ((peak - equity) / peak) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        if let Err(e) = repo
            .insert(user_id, &exchange, peak, equity, dd_pct)
            .await
        {
            warn!(%e, "v2_drawdown_snapshot: insert failed");
        }
    }
}
