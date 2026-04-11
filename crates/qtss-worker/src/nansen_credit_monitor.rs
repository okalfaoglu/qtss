//! Nansen API credit monitor loop.
//!
//! Periodically reads `meta_json` from `data_snapshots` for all Nansen
//! source keys, extracts `x_nansen_credits_remaining`, and fires
//! notifications via `notify_outbox` when credits drop below configured
//! thresholds. All tunables live in `system_config` (CLAUDE.md #2).
//!
//! Thresholds (configurable):
//! - **warning** (default 20%): first alert — "credits running low"
//! - **critical** (default 5%): urgent — "credits almost depleted"
//! - **exhausted** (0 or 403 insufficient_credits): "credits exhausted"
//!
//! Deduplication: each severity level is only notified once per
//! `cooldown_secs` window (default 6h) to avoid spam.

use sqlx::PgPool;
use std::time::Duration;
use tracing::{debug, info, warn};

use qtss_storage::{
    resolve_system_f64, resolve_system_string, resolve_worker_enabled_flag,
    resolve_worker_tick_secs, NotifyOutboxRepository,
};

/// Source keys whose `meta_json` may contain Nansen credit headers.
const NANSEN_SOURCE_KEYS: &[&str] = &[
    "nansen_netflows",
    "nansen_holdings",
    "nansen_perp_trades",
    "nansen_flow_intelligence",
    "nansen_smart_money_dex_trades",
    "nansen_token_screener",
    "nansen_perp_leaderboard",
    "nansen_whale_perp_aggregate",
];

const EVENT_KEY: &str = "nansen_credit_alert";

pub async fn nansen_credit_monitor_loop(pool: PgPool) {
    info!("nansen_credit_monitor loop spawned");
    loop {
        if !resolve_worker_enabled_flag(
            &pool,
            "monitoring",
            "nansen_credit_check_enabled",
            "QTSS_NANSEN_CREDIT_MONITOR",
            true,
        )
        .await
        {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }

        let tick = resolve_worker_tick_secs(
            &pool,
            "monitoring",
            "nansen_credit_check_tick_secs",
            "QTSS_NANSEN_CREDIT_CHECK_TICK_SECS",
            900,  // default 15 min
            300,  // min 5 min
        )
        .await;

        if let Err(e) = check_credits(&pool).await {
            warn!(%e, "nansen_credit_monitor check failed");
        }

        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}

async fn check_credits(pool: &PgPool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Read thresholds from config
    let warn_pct = resolve_system_f64(
        pool,
        "monitoring",
        "nansen_credit_warn_pct",
        "QTSS_NANSEN_CREDIT_WARN_PCT",
        20.0,
    )
    .await;
    let critical_pct = resolve_system_f64(
        pool,
        "monitoring",
        "nansen_credit_critical_pct",
        "QTSS_NANSEN_CREDIT_CRITICAL_PCT",
        5.0,
    )
    .await;
    let cooldown_secs = resolve_system_f64(
        pool,
        "monitoring",
        "nansen_credit_alert_cooldown_secs",
        "QTSS_NANSEN_CREDIT_COOLDOWN_SECS",
        21600.0, // 6 hours
    )
    .await as i64;
    let channels_csv = resolve_system_string(
        pool,
        "monitoring",
        "nansen_credit_alert_channels",
        "QTSS_NANSEN_CREDIT_CHANNELS",
        "telegram",
    )
    .await;
    let channels: Vec<String> = channels_csv
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Scan all Nansen source keys for the freshest credit reading
    let mut best_remaining: Option<f64> = None;
    let mut best_used: Option<f64> = None;
    let mut any_insufficient = false;
    let mut freshest_key: Option<String> = None;

    for key in NANSEN_SOURCE_KEYS {
        let row = match qtss_storage::data_snapshots::fetch_data_snapshot(pool, key).await {
            Ok(Some(r)) => r,
            _ => continue,
        };

        // Check insufficient credits flag
        if let Some(meta) = &row.meta_json {
            if meta
                .get("nansen_insufficient_credits")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                any_insufficient = true;
            }

            if let Some(rem_str) = meta
                .get("x_nansen_credits_remaining")
                .and_then(|v| v.as_str())
            {
                if let Ok(rem) = rem_str.parse::<f64>() {
                    if best_remaining.map(|b| rem < b).unwrap_or(true) {
                        best_remaining = Some(rem);
                        freshest_key = Some(key.to_string());
                    }
                }
            }
            if let Some(used_str) = meta
                .get("x_nansen_credits_used")
                .and_then(|v| v.as_str())
            {
                if let Ok(u) = used_str.parse::<f64>() {
                    if best_used.map(|b| u > b).unwrap_or(true) {
                        best_used = Some(u);
                    }
                }
            }
        }
    }

    // Determine severity
    let (remaining, used) = match (best_remaining, best_used) {
        (Some(r), u) => (r, u.unwrap_or(0.0)),
        (None, _) if any_insufficient => {
            // No remaining header but got 403 insufficient
            fire_alert(
                pool,
                "critical",
                "Nansen Kredisi Tukendi",
                "Nansen API 403 Insufficient Credits dondu. Tum chain pillar verileri durdu.",
                &channels,
                cooldown_secs,
            )
            .await;
            return Ok(());
        }
        _ => {
            debug!("nansen_credit_monitor: no credit data available yet");
            return Ok(());
        }
    };

    let total = remaining + used;
    if total <= 0.0 {
        debug!("nansen_credit_monitor: total credits = 0, skipping");
        return Ok(());
    }

    let pct_remaining = (remaining / total) * 100.0;

    info!(
        remaining = remaining,
        used = used,
        total = total,
        pct_remaining = format!("{:.1}%", pct_remaining),
        source = freshest_key.as_deref().unwrap_or("?"),
        "nansen credit check"
    );

    if remaining <= 0.0 || any_insufficient {
        fire_alert(
            pool,
            "critical",
            "Nansen Kredisi Tukendi!",
            &format!(
                "Kalan: {:.0} / {:.0} ({:.1}%)\nChain pillar verileri durdu. Nansen hesabini kontrol et.",
                remaining, total, pct_remaining
            ),
            &channels,
            cooldown_secs,
        )
        .await;
    } else if pct_remaining <= critical_pct {
        fire_alert(
            pool,
            "critical",
            "Nansen Kredisi Kritik Seviyede!",
            &format!(
                "Kalan: {:.0} / {:.0} ({:.1}%)\nKritik esik: %{:.0}. Kredi yuklemesi gerekiyor.",
                remaining, total, pct_remaining, critical_pct
            ),
            &channels,
            cooldown_secs,
        )
        .await;
    } else if pct_remaining <= warn_pct {
        fire_alert(
            pool,
            "warning",
            "Nansen Kredisi Azaliyor",
            &format!(
                "Kalan: {:.0} / {:.0} ({:.1}%)\nUyari esigi: %{:.0}.",
                remaining, total, pct_remaining, warn_pct
            ),
            &channels,
            cooldown_secs,
        )
        .await;
    } else {
        debug!(
            pct_remaining = format!("{:.1}%", pct_remaining),
            "nansen credits OK"
        );
    }

    Ok(())
}

async fn fire_alert(
    pool: &PgPool,
    severity: &str,
    title: &str,
    body: &str,
    channels: &[String],
    cooldown_secs: i64,
) {
    let repo = NotifyOutboxRepository::new(pool.clone());

    // Deduplicate: skip if same event_key + severity fired recently
    let dedupe_key = format!("{EVENT_KEY}_{severity}");
    match repo
        .exists_recent_global_event_symbol(&dedupe_key, "NANSEN", cooldown_secs)
        .await
    {
        Ok(true) => {
            debug!(
                severity,
                cooldown_secs, "nansen credit alert suppressed (cooldown)"
            );
            return;
        }
        Ok(false) => {}
        Err(e) => {
            warn!(%e, "nansen credit alert dedupe check failed");
        }
    }

    // Log
    match severity {
        "critical" => {
            warn!(title, body, "NANSEN CREDIT ALERT");
            qtss_common::log_critical("nansen_credit_monitor", &format!("{title}: {body}"));
        }
        _ => {
            warn!(title, body, "nansen credit warning");
        }
    }

    // Enqueue notification
    if let Err(e) = repo
        .enqueue_with_meta(
            None,
            Some(&dedupe_key),
            severity,
            None,
            None,
            Some("NANSEN"),
            title,
            body,
            channels.to_vec(),
        )
        .await
    {
        warn!(%e, "nansen credit alert enqueue failed");
    } else {
        info!(severity, title, "nansen credit alert enqueued");
    }
}
