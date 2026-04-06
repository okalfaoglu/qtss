//! Auto-promote intake playbook candidates to `engine_symbols` with lifecycle state management.
//!
//! Reads un-promoted candidates from `intake_playbook_candidates`, filters by confidence
//! and allowed playbook IDs, inserts into `engine_symbols` with `lifecycle_state = 'analyzing'`
//! and `enabled = true` (light pipeline via `intake:` label prefix).
//!
//! Enable: `system_config` `worker.intake_auto_promote_enabled` (default off).

use sqlx::PgPool;
use tracing::{debug, info, warn};

use qtss_storage::{
    count_engine_symbols_by_lifecycle, fetch_engine_symbol_by_series, fetch_intake_playbook_run_by_id,
    insert_engine_symbol, list_promotable_intake_candidates, resolve_system_csv, resolve_system_string,
    resolve_worker_enabled_flag, resolve_worker_tick_secs, update_engine_symbol_lifecycle_and_enabled,
    update_intake_candidate_merged_engine_symbol, EngineSymbolInsert, NotifyOutboxRepository,
};

fn normalize_symbol(raw: &str) -> String {
    let u = raw.trim().to_uppercase();
    if u.is_empty() {
        return u;
    }
    if u.ends_with("USDT") || u.ends_with("USDC") || u.ends_with("BUSD") {
        u
    } else {
        format!("{u}USDT")
    }
}

fn direction_mode(direction: &str) -> Option<String> {
    match direction.trim().to_uppercase().as_str() {
        "LONG" | "WATCH" => Some("long_only".into()),
        "SHORT" | "AVOID" => Some("short_only".into()),
        "LONG_OR_SHORT" => Some("both".into()),
        _ => Some("both".into()),
    }
}

async fn run_auto_promote(pool: &PgPool) -> Result<u32, qtss_storage::StorageError> {
    let active_states = &[
        "promoted",
        "analyzing",
        "ready",
        "trading",
        "closing",
    ];
    let max_active_str = resolve_system_string(
        pool,
        "worker",
        "intake_auto_promote_max_active",
        "QTSS_INTAKE_AUTO_PROMOTE_MAX_ACTIVE",
        "20",
    )
    .await;
    let max_active: i64 = max_active_str.parse().unwrap_or(20);

    let current_active = count_engine_symbols_by_lifecycle(pool, active_states).await?;
    if current_active >= max_active {
        debug!(current = current_active, max = max_active, "auto-promote: max active reached");
        return Ok(0);
    }
    let slots = (max_active - current_active).max(0) as i64;

    let min_conf_str = resolve_system_string(
        pool,
        "worker",
        "intake_auto_promote_min_confidence",
        "QTSS_INTAKE_AUTO_PROMOTE_MIN_CONFIDENCE",
        "60",
    )
    .await;
    let min_confidence: i32 = min_conf_str.parse().unwrap_or(60);

    let playbooks_csv = resolve_system_csv(
        pool,
        "worker",
        "intake_auto_promote_playbooks",
        "QTSS_INTAKE_AUTO_PROMOTE_PLAYBOOKS",
        "elite_long,elite_short,ten_x_alert,institutional_accumulation,institutional_exit",
    )
    .await;
    let playbook_ids: Vec<String> = playbooks_csv
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let default_interval = resolve_system_string(
        pool,
        "worker",
        "intake_auto_promote_default_interval",
        "QTSS_INTAKE_AUTO_PROMOTE_DEFAULT_INTERVAL",
        "15m",
    )
    .await;

    let candidates = list_promotable_intake_candidates(pool, &playbook_ids, min_confidence, slots).await?;
    if candidates.is_empty() {
        return Ok(0);
    }

    let mut promoted_count = 0_u32;
    let mut promoted_symbols = Vec::new();

    for c in &candidates {
        let sym = normalize_symbol(&c.symbol);
        if sym.is_empty() {
            continue;
        }

        let run = fetch_intake_playbook_run_by_id(pool, c.run_id).await?;
        let playbook_id = run
            .as_ref()
            .map(|r| r.playbook_id.as_str())
            .unwrap_or("unknown");

        let existing = fetch_engine_symbol_by_series(pool, "binance", "futures", &sym, &default_interval).await?;
        if let Some(es) = existing {
            if es.lifecycle_state != "retired" && es.lifecycle_state != "manual" {
                update_intake_candidate_merged_engine_symbol(pool, c.id, es.id).await?;
                debug!(symbol = %sym, state = %es.lifecycle_state, "auto-promote: already active, linked");
                continue;
            }
            if es.lifecycle_state == "retired" {
                update_engine_symbol_lifecycle_and_enabled(pool, es.id, "analyzing", true).await?;
                update_intake_candidate_merged_engine_symbol(pool, c.id, es.id).await?;
                promoted_count += 1;
                promoted_symbols.push(sym.clone());
                info!(symbol = %sym, id = %es.id, "auto-promote: re-activated retired symbol");
                continue;
            }
        }

        let mode = direction_mode(&c.direction);
        let label = Some(format!("intake:{playbook_id}"));
        let ins = EngineSymbolInsert {
            exchange: "binance".into(),
            segment: "futures".into(),
            symbol: sym.clone(),
            interval: default_interval.clone(),
            label,
            signal_direction_mode: mode,
        };
        let es = insert_engine_symbol(pool, &ins).await?;
        update_engine_symbol_lifecycle_and_enabled(pool, es.id, "analyzing", true).await?;
        update_intake_candidate_merged_engine_symbol(pool, c.id, es.id).await?;
        promoted_count += 1;
        promoted_symbols.push(sym);
        info!(id = %es.id, playbook = %playbook_id, "auto-promote: new engine_symbol created");
    }

    if promoted_count > 0 {
        let notify_enabled = resolve_worker_enabled_flag(
            pool,
            "worker",
            "intake_playbook_notify_enabled",
            "QTSS_INTAKE_PLAYBOOK_NOTIFY_ENABLED",
            false,
        )
        .await;
        if notify_enabled {
            let channels = resolve_system_csv(
                pool,
                "worker",
                "intake_playbook_notify_channels",
                "QTSS_INTAKE_PLAYBOOK_NOTIFY_CHANNELS",
                "telegram",
            )
            .await
            .into_iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
            if !channels.is_empty() {
                let repo = NotifyOutboxRepository::new(pool.clone());
                let title = format!("Auto-promote: {promoted_count} symbol(s)");
                let body = format!("Symbols: {}", promoted_symbols.join(", "));
                if let Err(e) = repo
                    .enqueue_with_meta(
                        None,
                        Some("intake_auto_promote"),
                        "info",
                        None,
                        None,
                        None,
                        &title,
                        &body,
                        channels,
                    )
                    .await
                {
                    warn!(%e, "auto-promote notification failed");
                }
            }
        }
    }

    Ok(promoted_count)
}

pub async fn intake_auto_promote_loop(pool: PgPool) {
    info!("intake_auto_promote: loop started");
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "intake_auto_promote_enabled",
            "QTSS_INTAKE_AUTO_PROMOTE_ENABLED",
            false,
        )
        .await;
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "intake_auto_promote_tick_secs",
            "QTSS_INTAKE_AUTO_PROMOTE_TICK_SECS",
            120,
            30,
        )
        .await;

        if enabled {
            if qtss_common::kill_switch::is_trading_halted() {
                debug!("auto-promote: kill switch active, skipping");
            } else {
                match run_auto_promote(&pool).await {
                    Ok(n) => {
                        if n > 0 {
                            info!(promoted = n, "auto-promote sweep ok");
                        }
                    }
                    Err(e) => warn!(%e, "auto-promote sweep failed"),
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(tick)).await;
    }
}
