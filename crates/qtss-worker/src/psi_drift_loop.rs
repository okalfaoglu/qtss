//! PSI drift circuit breaker — playbook `FAZ_9B_DRIFT_RUNBOOK` §2.
//!
//! Every `ai.drift.check_interval_secs` the loop:
//!   1. Calls the Python sidecar `/drift/psi` which computes PSI per
//!      feature (training dist vs live last `psi_lookback_hours`) and
//!      writes rows to `qtss_ml_drift_snapshots`.
//!   2. Counts features at `critical` status.
//!   3. If `critical_count ≥ critical_features_for_trip` AND no
//!      unresolved breaker event is already open for the active model,
//!      inserts a row into `qtss_ml_breaker_events`, runs the configured
//!      action (`deactivate` | `alert_only` | `throttle`), and drops an
//!      operator alert into `notify_outbox`.
//!
//! CLAUDE.md:
//!   #1 breaker action chosen via dispatch (no nested match ladders in the hot path)
//!   #2 every threshold sourced from `system_config.ai.drift.*`
//!   #4 loop is agnostic of what a "feature" is — only reads names + psi

use std::time::Duration;

use qtss_storage::{
    resolve_system_string, resolve_system_u64, resolve_worker_enabled_flag, NotifyOutboxRepository,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, info, warn};

const MODULE: &str = "ai";
const CFG_ENABLED: &str = "drift.enabled";
const CFG_CHECK_SECS: &str = "drift.check_interval_secs";
// The psi_critical_threshold is read by the sidecar (bucket classifier
// returns status="critical"); kept here as doc anchor so operators know
// where to tune.
#[allow(dead_code)]
const CFG_PSI_CRITICAL: &str = "drift.psi_critical_threshold";
const CFG_CRITICAL_FOR_TRIP: &str = "drift.critical_features_for_trip";
const CFG_BREAKER_ACTION: &str = "drift.breaker_action";
const CFG_SIDECAR_URL: &str = "inference.sidecar_url";

const ENV_ENABLED: &str = "QTSS_DRIFT_ENABLED";
const ENV_CHECK_SECS: &str = "QTSS_DRIFT_CHECK_SECS";
const ENV_SIDECAR_URL: &str = "QTSS_AI_INFERENCE_SIDECAR_URL";

const DEFAULT_CHECK_SECS: u64 = 1_800;
const DEFAULT_CRIT_FOR_TRIP: u64 = 3;
const DEFAULT_BREAKER_ACTION: &str = "deactivate";
const DEFAULT_SIDECAR_URL: &str = "http://127.0.0.1:8790";

// Status strings the sidecar emits (see server.py `/drift/psi`).
const STATUS_CRITICAL: &str = "critical";

#[derive(Debug, Deserialize)]
struct DriftFeatureEntry {
    feature: String,
    psi: f64,
    status: String,
    #[serde(default, rename = "buckets")]
    _buckets: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct DriftResponse {
    features: Vec<DriftFeatureEntry>,
    model_version: String,
    #[serde(default, rename = "computed_at")]
    _computed_at: String,
}

pub async fn psi_drift_loop(pool: PgPool) {
    info!("psi drift loop: starting (runbook §2)");
    loop {
        let enabled = resolve_worker_enabled_flag(&pool, MODULE, CFG_ENABLED, ENV_ENABLED, true).await;
        let check_secs = resolve_system_u64(
            &pool, MODULE, CFG_CHECK_SECS, ENV_CHECK_SECS,
            DEFAULT_CHECK_SECS, 60, 24 * 3600,
        )
        .await;
        if enabled {
            if let Err(e) = tick(&pool).await {
                warn!(error = %e, "psi drift loop: tick failed");
            }
        }
        tokio::time::sleep(Duration::from_secs(check_secs)).await;
    }
}

async fn tick(pool: &PgPool) -> Result<(), String> {
    let sidecar_url = resolve_system_string(
        pool, MODULE, CFG_SIDECAR_URL, ENV_SIDECAR_URL, DEFAULT_SIDECAR_URL,
    )
    .await;
    let crit_threshold = resolve_system_u64(
        pool, MODULE, CFG_CRITICAL_FOR_TRIP, "",
        DEFAULT_CRIT_FOR_TRIP, 1, 10_000,
    )
    .await as usize;
    let action = resolve_system_string(
        pool, MODULE, CFG_BREAKER_ACTION, "", DEFAULT_BREAKER_ACTION,
    )
    .await;

    // 1. Pull PSI snapshot from sidecar (which also persists it).
    let resp = fetch_psi(&sidecar_url).await?;
    debug!(
        n_features = resp.features.len(),
        model_version = %resp.model_version,
        "psi drift loop: snapshot"
    );

    // 2. Collect critical features.
    let critical: Vec<&DriftFeatureEntry> = resp.features.iter()
        .filter(|f| f.status == STATUS_CRITICAL)
        .collect();
    if critical.len() < crit_threshold {
        debug!(
            critical = critical.len(),
            threshold = crit_threshold,
            "psi drift loop: below trip threshold"
        );
        return Ok(());
    }

    // 3. Short-circuit if a breaker event is already open for this model.
    let model_id_opt = fetch_active_model_id(pool, &resp.model_version).await;
    let Some(model_id) = model_id_opt else {
        warn!(
            model_version = %resp.model_version,
            "psi drift loop: no qtss_models row matches sidecar's active version; skip trip"
        );
        return Ok(());
    };
    if has_open_breaker(pool, model_id).await {
        debug!(%model_id, "psi drift loop: breaker already open, no double-fire");
        return Ok(());
    }

    // 4. Trip: insert breaker event + run action + notify.
    let reason = format!(
        "{} features PSI ≥ critical ({}): {}",
        critical.len(),
        crit_threshold,
        critical.iter().take(5).map(|f| f.feature.as_str()).collect::<Vec<_>>().join(", "),
    );
    let critical_json = json!(critical.iter()
        .map(|f| json!({ "feature": f.feature, "psi": f.psi }))
        .collect::<Vec<_>>());

    let event_id = insert_breaker_event(pool, model_id, &action, &reason, &critical_json).await?;
    run_action(pool, model_id, &action).await;
    alert_operator(pool, &action, &reason, &resp.model_version, event_id).await;

    info!(action = %action, reason = %reason, "psi drift loop: breaker tripped");
    Ok(())
}

async fn fetch_psi(sidecar_url: &str) -> Result<DriftResponse, String> {
    let url = format!("{}/drift/psi", sidecar_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("client build: {e}"))?;
    let resp = client.get(&url).send().await
        .map_err(|e| format!("sidecar {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("sidecar {} non-2xx: {}", url, resp.status()));
    }
    resp.json::<DriftResponse>().await
        .map_err(|e| format!("parse drift response: {e}"))
}

async fn fetch_active_model_id(pool: &PgPool, model_version: &str) -> Option<uuid::Uuid> {
    // Prefer the model the sidecar says is active; fall back to DB-active row
    // in case the sidecar cache is stale right after a reload.
    if !model_version.is_empty() {
        if let Ok(Some((id,))) = sqlx::query_as::<_, (uuid::Uuid,)>(
            "SELECT id FROM qtss_models WHERE model_version = $1 ORDER BY trained_at DESC LIMIT 1",
        )
        .bind(model_version)
        .fetch_optional(pool)
        .await
        {
            return Some(id);
        }
    }
    sqlx::query_as::<_, (uuid::Uuid,)>(
        "SELECT id FROM qtss_models WHERE active = true ORDER BY trained_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|(id,)| id)
}

async fn has_open_breaker(pool: &PgPool, model_id: uuid::Uuid) -> bool {
    sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM qtss_ml_breaker_events
          WHERE model_id = $1 AND resolved_at IS NULL",
    )
    .bind(model_id)
    .fetch_one(pool)
    .await
    .map(|(n,)| n > 0)
    .unwrap_or(false)
}

async fn insert_breaker_event(
    pool: &PgPool,
    model_id: uuid::Uuid,
    action: &str,
    reason: &str,
    critical_features: &serde_json::Value,
) -> Result<uuid::Uuid, String> {
    let row = sqlx::query_as::<_, (uuid::Uuid,)>(
        r#"INSERT INTO qtss_ml_breaker_events (model_id, action, reason, critical_features)
           VALUES ($1, $2, $3, $4::jsonb)
           RETURNING id"#,
    )
    .bind(model_id)
    .bind(action)
    .bind(reason)
    .bind(critical_features)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("insert breaker event: {e}"))?;
    Ok(row.0)
}

// ── Action dispatch (CLAUDE.md #1) ─────────────────────────────────────────
//
// Three actions today; adding a fourth = one row in the match + one function.
async fn run_action(pool: &PgPool, model_id: uuid::Uuid, action: &str) {
    match action {
        "deactivate" => action_deactivate(pool, model_id).await,
        "alert_only" => action_alert_only(model_id),
        "throttle"   => action_throttle(pool).await,
        other => warn!(action = %other, "psi drift loop: unknown breaker action, treating as alert_only"),
    }
}

async fn action_deactivate(pool: &PgPool, model_id: uuid::Uuid) {
    let r = sqlx::query("UPDATE qtss_models SET active = false WHERE id = $1")
        .bind(model_id)
        .execute(pool)
        .await;
    match r {
        Ok(_) => info!(%model_id, "breaker action: model deactivated"),
        Err(e) => warn!(%model_id, error = %e, "breaker action: deactivate failed"),
    }
}

fn action_alert_only(model_id: uuid::Uuid) {
    info!(%model_id, "breaker action: alert_only (no state mutation)");
}

async fn action_throttle(pool: &PgPool) {
    // Flip the inference gate so setup engine scores shadow-only until
    // operator resolves the breaker. Safer than deactivation because the
    // AI is still observable for post-mortem.
    let r = sqlx::query(
        r#"INSERT INTO system_config (module, config_key, value, description)
           VALUES ('ai', 'inference.gate_enabled', 'false'::jsonb,
                   'Auto-throttled by PSI breaker')
           ON CONFLICT (module, config_key) DO UPDATE SET value = 'false'::jsonb"#,
    )
    .execute(pool)
    .await;
    match r {
        Ok(_) => info!("breaker action: throttle — inference.gate_enabled=false"),
        Err(e) => warn!(error = %e, "breaker action: throttle failed"),
    }
}

async fn alert_operator(
    pool: &PgPool,
    action: &str,
    reason: &str,
    model_version: &str,
    event_id: uuid::Uuid,
) {
    let title = format!("PSI breaker tripped ({})", action);
    let body = format!(
        "Model: <code>{mv}</code>\nReason: {reason}\nEvent: <code>{eid}</code>\nRunbook: docs/FAZ_9B_DRIFT_RUNBOOK.md",
        mv = model_version,
        reason = reason,
        eid = event_id,
    );
    let repo = NotifyOutboxRepository::new(pool.clone());
    if let Err(e) = repo
        .enqueue_with_meta(
            None,
            Some("ai_psi_breaker"),
            "critical",
            None,
            None,
            None,
            &title,
            &body,
            vec!["telegram".to_string()],
        )
        .await
    {
        warn!(error = %e, "psi drift loop: notify_outbox enqueue failed");
    }
}
