//! Lifecycle state machine for `engine_symbols` managed by the intake pipeline.
//!
//! Observes current state of each non-manual/non-retired `engine_symbols` row and advances
//! through the lifecycle: `promoted -> analyzing -> ready -> trading -> closing -> cooldown -> retired`.
//!
//! Does NOT modify other workers' behavior — pure observer pattern.
//! Enable: `system_config` `worker.lifecycle_manager_enabled` (default off).

use sqlx::PgPool;
use tracing::{debug, info, warn};

use qtss_storage::{
    has_analysis_snapshots, has_applied_tactical_for_symbol, list_engine_symbols_by_lifecycle,
    list_stale_lifecycle_engine_symbols, net_filled_position_for_symbol, resolve_system_string,
    resolve_worker_enabled_flag, resolve_worker_tick_secs, update_engine_symbol_lifecycle_and_enabled,
    update_engine_symbol_lifecycle_state, EngineSymbolRow,
};

async fn advance_promoted(pool: &PgPool) -> Result<u32, qtss_storage::StorageError> {
    let rows = list_engine_symbols_by_lifecycle(pool, &["promoted"]).await?;
    let mut count = 0;
    for r in &rows {
        update_engine_symbol_lifecycle_and_enabled(pool, r.id, "analyzing", true).await?;
        info!(id = %r.id, symbol = %r.symbol, "lifecycle: promoted -> analyzing");
        count += 1;
    }
    Ok(count)
}

async fn advance_analyzing(pool: &PgPool) -> Result<u32, qtss_storage::StorageError> {
    let rows = list_engine_symbols_by_lifecycle(pool, &["analyzing"]).await?;
    let mut count = 0;
    for r in &rows {
        let has_tr = has_analysis_snapshots(pool, r.id, &["trading_range"]).await?;
        let has_tbm = has_analysis_snapshots(pool, r.id, &["tbm_scores"]).await?;
        if has_tr && has_tbm {
            update_engine_symbol_lifecycle_state(pool, r.id, "ready").await?;
            info!(id = %r.id, symbol = %r.symbol, "lifecycle: analyzing -> ready");
            count += 1;
        }
    }
    Ok(count)
}

async fn advance_ready(pool: &PgPool) -> Result<u32, qtss_storage::StorageError> {
    let rows = list_engine_symbols_by_lifecycle(pool, &["ready"]).await?;
    let mut count = 0;
    for r in &rows {
        if has_applied_tactical_for_symbol(pool, &r.symbol).await? {
            update_engine_symbol_lifecycle_state(pool, r.id, "trading").await?;
            info!(id = %r.id, symbol = %r.symbol, "lifecycle: ready -> trading");
            count += 1;
        }
    }
    Ok(count)
}

async fn advance_trading(pool: &PgPool) -> Result<u32, qtss_storage::StorageError> {
    let rows = list_engine_symbols_by_lifecycle(pool, &["trading"]).await?;
    let mut count = 0;
    for r in &rows {
        let net_pos = net_filled_position_for_symbol(pool, &r.symbol).await?;
        if net_pos.abs() < 1e-9 {
            update_engine_symbol_lifecycle_state(pool, r.id, "closing").await?;
            info!(id = %r.id, symbol = %r.symbol, "lifecycle: trading -> closing (flat)");
            count += 1;
        }
    }
    Ok(count)
}

async fn advance_closing(pool: &PgPool) -> Result<u32, qtss_storage::StorageError> {
    let rows = list_engine_symbols_by_lifecycle(pool, &["closing"]).await?;
    let mut count = 0;
    for r in &rows {
        let net_pos = net_filled_position_for_symbol(pool, &r.symbol).await?;
        if net_pos.abs() < 1e-9 {
            update_engine_symbol_lifecycle_state(pool, r.id, "cooldown").await?;
            info!(id = %r.id, symbol = %r.symbol, "lifecycle: closing -> cooldown");
            count += 1;
        }
    }
    Ok(count)
}

async fn advance_cooldown(pool: &PgPool, cooldown_hours: i64) -> Result<u32, qtss_storage::StorageError> {
    let rows = list_engine_symbols_by_lifecycle(pool, &["cooldown"]).await?;
    let mut count = 0;
    let threshold = chrono::Utc::now() - chrono::Duration::hours(cooldown_hours);
    for r in &rows {
        if r.updated_at < threshold {
            update_engine_symbol_lifecycle_and_enabled(pool, r.id, "retired", false).await?;
            info!(id = %r.id, symbol = %r.symbol, "lifecycle: cooldown -> retired");
            count += 1;
        }
    }
    Ok(count)
}

async fn retire_stale(pool: &PgPool, stale_hours: i64) -> Result<u32, qtss_storage::StorageError> {
    let rows = list_stale_lifecycle_engine_symbols(pool, stale_hours).await?;
    let mut count = 0;
    for r in &rows {
        let net_pos = net_filled_position_for_symbol(pool, &r.symbol).await?;
        if net_pos.abs() > 1e-9 {
            debug!(id = %r.id, symbol = %r.symbol, net = net_pos, "stale but has position, skipping retire");
            continue;
        }
        update_engine_symbol_lifecycle_and_enabled(pool, r.id, "retired", false).await?;
        info!(id = %r.id, symbol = %r.symbol, state = %r.lifecycle_state, "lifecycle: stale -> retired");
        count += 1;
    }
    Ok(count)
}

async fn run_lifecycle_sweep(pool: &PgPool) -> Result<(), qtss_storage::StorageError> {
    let cooldown_str = resolve_system_string(
        pool,
        "worker",
        "lifecycle_cooldown_hours",
        "QTSS_LIFECYCLE_COOLDOWN_HOURS",
        "24",
    )
    .await;
    let cooldown_hours: i64 = cooldown_str.parse().unwrap_or(24);

    let stale_str = resolve_system_string(
        pool,
        "worker",
        "lifecycle_retire_stale_hours",
        "QTSS_LIFECYCLE_RETIRE_STALE_HOURS",
        "48",
    )
    .await;
    let stale_hours: i64 = stale_str.parse().unwrap_or(48);

    let p = advance_promoted(pool).await?;
    let a = advance_analyzing(pool).await?;
    let r = advance_ready(pool).await?;
    let t = advance_trading(pool).await?;
    let cl = advance_closing(pool).await?;
    let cd = advance_cooldown(pool, cooldown_hours).await?;
    let st = retire_stale(pool, stale_hours).await?;

    let total = p + a + r + t + cl + cd + st;
    if total > 0 {
        info!(
            promoted = p,
            analyzing = a,
            ready = r,
            trading = t,
            closing = cl,
            cooldown = cd,
            stale = st,
            "lifecycle sweep: {total} transitions"
        );
    }

    Ok(())
}

pub async fn lifecycle_manager_loop(pool: PgPool) {
    info!("lifecycle_manager: loop started");
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "lifecycle_manager_enabled",
            "QTSS_LIFECYCLE_MANAGER_ENABLED",
            false,
        )
        .await;
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "lifecycle_manager_tick_secs",
            "QTSS_LIFECYCLE_MANAGER_TICK_SECS",
            300,
            60,
        )
        .await;

        if enabled {
            match run_lifecycle_sweep(&pool).await {
                Ok(()) => {}
                Err(e) => warn!(%e, "lifecycle sweep failed"),
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(tick)).await;
    }
}
