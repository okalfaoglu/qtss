//! Nansen-driven `engine_symbols` lifecycle management.
//!
//! Two responsibilities:
//! 1. **Promote**: High-scoring `nansen_setup_rows` candidates → new `engine_symbols`
//! 2. **Disable**: Symbols that produce no Nansen enriched signals → `enabled = false`
//!
//! Enable: `system_config` `worker.nansen_symbol_lifecycle_enabled` (default off).

use sqlx::PgPool;
use tracing::{debug, info, warn};

use qtss_storage::{
    fetch_engine_symbol_by_series, insert_engine_symbol, is_binance_futures_tradable,
    list_enabled_engine_symbols, resolve_system_f64, resolve_system_string, resolve_system_u64,
    resolve_worker_enabled_flag, resolve_worker_tick_secs, update_engine_symbol_lifecycle_and_enabled,
    EngineSymbolInsert, NotifyOutboxRepository,
};

// ── Promote: nansen_setup_rows → engine_symbols ──────────────────────

async fn run_nansen_promote(pool: &PgPool) -> Result<u32, qtss_storage::StorageError> {
    let min_score: f64 = resolve_system_f64(
        pool,
        "worker",
        "nansen_promote.min_score",
        "QTSS_NANSEN_PROMOTE_MIN_SCORE",
        80.0,
    )
    .await;

    let max_active = resolve_system_u64(
        pool,
        "worker",
        "nansen_promote.max_active",
        "QTSS_NANSEN_PROMOTE_MAX_ACTIVE",
        20,
        1,
        100,
    )
    .await as i64;

    let default_interval = resolve_system_string(
        pool,
        "worker",
        "nansen_promote.default_interval",
        "QTSS_NANSEN_PROMOTE_INTERVAL",
        "15m",
    )
    .await;

    let lookback_hours = resolve_system_u64(
        pool,
        "worker",
        "nansen_promote.lookback_hours",
        "QTSS_NANSEN_PROMOTE_LOOKBACK",
        24,
        1,
        168,
    )
    .await as i64;

    // Count current non-retired/non-manual symbols
    let current_active: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM engine_symbols
           WHERE enabled = true AND lifecycle_state NOT IN ('retired', 'manual')"#,
    )
    .fetch_one(pool)
    .await?;

    if current_active >= max_active {
        debug!(current = current_active, max = max_active, "nansen promote: max active reached");
        return Ok(0);
    }
    let slots = (max_active - current_active).max(0);

    // Fetch top nansen_setup_rows candidates from recent runs
    let candidates: Vec<NansenCandidate> = sqlx::query_as(
        r#"SELECT DISTINCT ON (r.token_symbol)
                  r.token_symbol, r.direction, r.score, r.chain,
                  r.token_address, r.probability
           FROM nansen_setup_rows r
           JOIN nansen_setup_runs run ON r.run_id = run.id
           WHERE run.computed_at > now() - make_interval(hours => $1)
             AND r.score >= $2
             AND r.token_symbol IS NOT NULL
             AND r.token_symbol != ''
           ORDER BY r.token_symbol, r.score DESC
           LIMIT $3"#,
    )
    .bind(lookback_hours)
    .bind(min_score as i32)
    .bind(slots)
    .fetch_all(pool)
    .await?;

    if candidates.is_empty() {
        return Ok(0);
    }

    let mut promoted = 0u32;
    let repo = NotifyOutboxRepository::new(pool.clone());

    for c in &candidates {
        let sym = normalize_nansen_symbol(&c.token_symbol);
        if sym.is_empty() {
            continue;
        }

        // Only promote if tradeable on Binance futures
        if !is_binance_futures_tradable(pool, &sym).await.unwrap_or(false) {
            debug!(symbol = %sym, raw = %c.token_symbol, "nansen promote: not on Binance futures");
            continue;
        }

        // Check if already exists
        let existing = fetch_engine_symbol_by_series(
            pool, "binance", "futures", &sym, &default_interval,
        )
        .await?;

        if let Some(es) = existing {
            if es.enabled && es.lifecycle_state != "retired" {
                debug!(symbol = %sym, state = %es.lifecycle_state, "nansen promote: already active");
                continue;
            }
            // Re-enable retired symbol
            if es.lifecycle_state == "retired" || !es.enabled {
                let mode = direction_mode(&c.direction);
                update_engine_symbol_lifecycle_and_enabled(pool, es.id, "analyzing", true).await?;
                info!(symbol = %sym, score = c.score, dir = %c.direction,
                      "nansen promote: re-enabled retired symbol");
                promoted += 1;

                let label = format!("nansen:score={}", c.score);
                sqlx::query(
                    "UPDATE engine_symbols SET label = $2, signal_direction_mode = COALESCE($3, signal_direction_mode), source = 'nansen_setup', updated_at = now() WHERE id = $1",
                )
                .bind(es.id)
                .bind(&label)
                .bind(&mode)
                .execute(pool)
                .await?;
            }
            continue;
        }

        // Create new engine_symbol
        let mode = direction_mode(&c.direction);
        let label = Some(format!("nansen:score={}", c.score));
        let ins = EngineSymbolInsert {
            exchange: "binance".into(),
            segment: "futures".into(),
            symbol: sym.clone(),
            interval: default_interval.clone(),
            label,
            signal_direction_mode: mode,
        };
        match insert_engine_symbol(pool, &ins).await {
            Ok(es) => {
                update_engine_symbol_lifecycle_and_enabled(pool, es.id, "analyzing", true).await?;
                // Tag source
                sqlx::query(
                    "UPDATE engine_symbols SET source = 'nansen_setup', discovered_at = now() WHERE id = $1",
                )
                .bind(es.id)
                .execute(pool)
                .await?;

                info!(symbol = %sym, score = c.score, dir = %c.direction, id = %es.id,
                      "nansen promote: new engine_symbol created");
                promoted += 1;
            }
            Err(e) => {
                warn!(symbol = %sym, %e, "nansen promote: insert failed");
            }
        }
    }

    if promoted > 0 {
        let _ = repo
            .enqueue_with_meta(
                None,
                Some("nansen_promote"),
                "info",
                None,
                None,
                None,
                "Nansen Auto-Promote",
                &format!("{promoted} yeni sembol engine_symbols'a eklendi"),
                vec![],
            )
            .await;
    }

    Ok(promoted)
}

// ── Disable: no-data symbols ─────────────────────────────────────────

async fn run_nansen_disable(pool: &PgPool) -> Result<u32, qtss_storage::StorageError> {
    let grace_hours = resolve_system_u64(
        pool,
        "worker",
        "nansen_disable.grace_hours",
        "QTSS_NANSEN_DISABLE_GRACE_HOURS",
        48,
        6,
        720,
    )
    .await as i64;

    let enabled_symbols = list_enabled_engine_symbols(pool).await?;
    let mut disabled = 0u32;
    let repo = NotifyOutboxRepository::new(pool.clone());

    for es in &enabled_symbols {
        // Skip pinned/manual symbols — user explicitly added
        if es.lifecycle_state == "manual" {
            // Manual symbols can also be disabled if no data, but
            // only if they're old enough (grace period).
            let age_hours = chrono::Utc::now()
                .signed_duration_since(es.created_at)
                .num_hours();
            if age_hours < grace_hours {
                continue;
            }
        }

        // Check: does this symbol have ANY enriched signal in recent history?
        let has_enriched: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                 SELECT 1 FROM nansen_enriched_signals
                 WHERE symbol = $1
                   AND computed_at > now() - make_interval(hours => $2)
               )"#,
        )
        .bind(&es.symbol)
        .bind(grace_hours)
        .fetch_one(pool)
        .await?;

        // Check: does this symbol have chain_score in onchain metrics?
        let has_chain: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                 SELECT 1 FROM qtss_v2_onchain_metrics
                 WHERE symbol = $1
                   AND chain_score IS NOT NULL
                   AND computed_at > now() - make_interval(hours => $2)
               )"#,
        )
        .bind(&es.symbol)
        .bind(grace_hours)
        .fetch_one(pool)
        .await?;

        // Check: does this symbol have derivatives data?
        let has_derivatives: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                 SELECT 1 FROM qtss_v2_onchain_metrics
                 WHERE symbol = $1
                   AND derivatives_score IS NOT NULL
                   AND computed_at > now() - make_interval(hours => $2)
               )"#,
        )
        .bind(&es.symbol)
        .bind(grace_hours)
        .fetch_one(pool)
        .await?;

        // If symbol has derivatives (Binance) data, it's still useful
        // even without Nansen chain data. Only disable if no data at all.
        if has_enriched || has_chain || has_derivatives {
            continue;
        }

        // No data at all → disable
        update_engine_symbol_lifecycle_and_enabled(pool, es.id, "retired", false).await?;
        info!(
            symbol = %es.symbol, id = %es.id, state = %es.lifecycle_state,
            "nansen disable: no data produced, disabled"
        );
        disabled += 1;

        let _ = repo
            .enqueue_with_meta(
                None,
                Some("nansen_disable"),
                "warning",
                None,
                None,
                Some(&es.symbol),
                &format!("{}: Veri Yok — Devre Dışı", es.symbol),
                &format!(
                    "{} sembolü {}+ saat boyunca enriched/chain/derivatives verisi üretmedi. Disable edildi.",
                    es.symbol, grace_hours
                ),
                vec![],
            )
            .await;
    }

    Ok(disabled)
}

// ── Public loop ──────────────────────────────────────────────────────

pub async fn nansen_symbol_lifecycle_loop(pool: PgPool) {
    let enabled = resolve_worker_enabled_flag(
        &pool,
        "worker",
        "nansen_symbol_lifecycle_enabled",
        "QTSS_NANSEN_SYMBOL_LIFECYCLE",
        false,
    )
    .await;

    if !enabled {
        info!("nansen_symbol_lifecycle loop disabled");
        return;
    }

    info!("nansen_symbol_lifecycle loop started");

    loop {
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "nansen_symbol_lifecycle_tick_secs",
            "QTSS_NANSEN_LIFECYCLE_TICK",
            3600,
            300,
        )
        .await;

        tokio::time::sleep(std::time::Duration::from_secs(tick)).await;

        match run_nansen_promote(&pool).await {
            Ok(n) if n > 0 => info!(promoted = n, "nansen lifecycle: promote pass done"),
            Ok(_) => debug!("nansen lifecycle: promote pass — nothing to promote"),
            Err(e) => warn!(%e, "nansen lifecycle: promote error"),
        }

        match run_nansen_disable(&pool).await {
            Ok(n) if n > 0 => info!(disabled = n, "nansen lifecycle: disable pass done"),
            Ok(_) => debug!("nansen lifecycle: disable pass — all symbols have data"),
            Err(e) => warn!(%e, "nansen lifecycle: disable error"),
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
struct NansenCandidate {
    token_symbol: String,
    direction: String,
    score: i32,
    chain: Option<String>,
    token_address: Option<String>,
    probability: Option<f64>,
}

fn normalize_nansen_symbol(raw: &str) -> String {
    let u = raw.trim().to_uppercase();
    // Remove emoji prefixes (🌱 etc.)
    let cleaned: String = u.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if cleaned.is_empty() {
        return String::new();
    }
    if cleaned.ends_with("USDT") || cleaned.ends_with("USDC") || cleaned.ends_with("BUSD") {
        cleaned
    } else {
        format!("{cleaned}USDT")
    }
}

fn direction_mode(direction: &str) -> Option<String> {
    match direction.trim().to_uppercase().as_str() {
        "LONG" | "WATCH" => Some("long_only".into()),
        "SHORT" | "AVOID" => Some("short_only".into()),
        _ => Some("both".into()),
    }
}
