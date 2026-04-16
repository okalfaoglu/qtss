//! Faz 9.3.3 — Rust client for the `qtss-trainer` inference sidecar.
//!
//! At setup-open time the D/T/Q loop assembles `features_by_source`,
//! forwards it to the sidecar, and stamps the returned P(win) on the
//! setup row (`qtss_v2_setups.ai_score`). The client is deliberately
//! thin: it owns no state, returns `None` on any soft failure
//! (disabled, unreachable, timeout, 5xx) so the setup path keeps
//! running in shadow mode until operators are confident.
//!
//! CLAUDE.md:
//!   * #2 — url / timeout / enabled / gate flag are config-driven
//!     (`ai.inference.*`); no constants in code.
//!   * #3 — detector/strategy/adapter boundaries stay intact; the
//!     client is a pure HTTP adapter and knows nothing about setups.
//!   * guard-style early returns, no nested if/else chains (#1).
//!
//! Shadow mode: `ai.inference.gate_enabled=false` by default. The
//! worker records the score but doesn't veto setups until the flag
//! flips. Once the Training Set Monitor confirms a calibrated score
//! distribution, operators enable the gate from Config Editor.

use std::time::Duration;

use qtss_storage::{resolve_system_f64, resolve_system_string, resolve_worker_enabled_flag};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use tracing::{debug, warn};

/// Resolved tunables — looked up once per setup-open so Config GUI
/// edits apply without a worker restart (CLAUDE.md #2).
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    pub enabled: bool,
    pub sidecar_url: String,
    pub timeout_ms: u64,
    pub gate_enabled: bool,
    pub min_score: f64,
    /// Faz 9.3.5 — when `gate_enabled=false`, still POST to the sidecar
    /// and persist the prediction so AI vs classic can be measured
    /// offline. Setting this to `false` is a hard kill-switch for
    /// sidecar traffic (skips both scoring and persistence).
    pub log_shadow_predictions: bool,
    /// Faz 9.3.4 — whether to also call `/explain` for top-10 SHAP.
    pub explain_enabled: bool,
}

impl InferenceConfig {
    pub async fn load(pool: &PgPool) -> Self {
        let enabled = resolve_worker_enabled_flag(
            pool,
            "ai",
            "inference.enabled",
            "QTSS_AI_INFERENCE_ENABLED",
            true,
        )
        .await;
        let sidecar_url = resolve_system_string(
            pool,
            "ai",
            "inference.sidecar_url",
            "QTSS_AI_INFERENCE_SIDECAR_URL",
            "http://127.0.0.1:8790",
        )
        .await;
        let timeout_ms = resolve_system_f64(
            pool,
            "ai",
            "inference.timeout_ms",
            "QTSS_AI_INFERENCE_TIMEOUT_MS",
            300.0,
        )
        .await as u64;
        let gate_enabled = resolve_worker_enabled_flag(
            pool,
            "ai",
            "inference.gate_enabled",
            "QTSS_AI_INFERENCE_GATE_ENABLED",
            false,
        )
        .await;
        let min_score = resolve_system_f64(
            pool,
            "ai",
            "inference.min_score",
            "QTSS_AI_INFERENCE_MIN_SCORE",
            0.55,
        )
        .await;
        let log_shadow_predictions = resolve_worker_enabled_flag(
            pool,
            "ai",
            "inference.log_shadow_predictions",
            "QTSS_AI_INFERENCE_LOG_SHADOW_PREDICTIONS",
            true,
        )
        .await;
        let explain_enabled = resolve_worker_enabled_flag(
            pool,
            "ai",
            "inference.explain_enabled",
            "QTSS_AI_INFERENCE_EXPLAIN_ENABLED",
            true,
        )
        .await;
        Self {
            enabled,
            sidecar_url,
            timeout_ms,
            gate_enabled,
            min_score,
            log_shadow_predictions,
            explain_enabled,
        }
    }
}

#[derive(Debug, Serialize)]
struct ScoreRequest<'a> {
    features_by_source: &'a JsonValue,
}

#[derive(Debug, Deserialize)]
pub struct ScoreResponse {
    pub score: f64,
    pub model_family: String,
    pub model_version: String,
    /// Faz 9.3.5 — logical model name; the sidecar currently mirrors
    /// `model_family` but kept separate so future A/B splits can diverge.
    #[serde(default)]
    pub model_name: String,
    #[serde(default)]
    pub feature_spec_version: String,
    #[serde(default)]
    pub missing_features: i64,
    #[serde(default)]
    pub n_features: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShapEntry {
    pub feature: String,
    pub value: f64,
    pub contribution: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExplainResponse {
    pub shap_top10: Vec<ShapEntry>,
    pub base_value: f64,
    #[serde(default)]
    pub model_version: String,
}

/// Fire one /score call. Returns `Ok(None)` for any *soft* failure
/// (disabled, sidecar unreachable, timeout, 5xx). Setup creation must
/// never block on inference — `None` just means the setup will be
/// persisted with `ai_score=NULL`.
pub async fn score(
    cfg: &InferenceConfig,
    features_by_source: &JsonValue,
) -> Option<ScoreResponse> {
    if !cfg.enabled {
        return None;
    }
    if features_by_source.is_null() {
        return None;
    }
    let url = format!("{}/score", cfg.sidecar_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(cfg.timeout_ms.max(50)))
        .build()
        .ok()?;

    let resp = match client
        .post(&url)
        .json(&ScoreRequest { features_by_source })
        .send()
        .await
    {
        Ok(r) => r,
        Err(err) => {
            debug!(%err, "inference sidecar unreachable (shadow mode keeps setup alive)");
            return None;
        }
    };

    if !resp.status().is_success() {
        warn!(status = %resp.status(), "inference sidecar non-2xx");
        return None;
    }
    match resp.json::<ScoreResponse>().await {
        Ok(body) => Some(body),
        Err(err) => {
            warn!(%err, "inference sidecar response parse failed");
            None
        }
    }
}

/// Faz 9.3.4 — Fire one `/explain` call. Same soft-fail contract as
/// `score`: returns `None` on any failure so the setup path never
/// blocks on SHAP extraction. This is diagnostic; persisting the
/// prediction itself is the caller's responsibility.
pub async fn explain(
    cfg: &InferenceConfig,
    features_by_source: &JsonValue,
) -> Option<ExplainResponse> {
    if !cfg.enabled || !cfg.explain_enabled {
        return None;
    }
    if features_by_source.is_null() {
        return None;
    }
    let url = format!("{}/explain", cfg.sidecar_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(cfg.timeout_ms.max(50)))
        .build()
        .ok()?;
    let resp = match client
        .post(&url)
        .json(&ScoreRequest { features_by_source })
        .send()
        .await
    {
        Ok(r) => r,
        Err(err) => {
            debug!(%err, "inference sidecar /explain unreachable");
            return None;
        }
    };
    if !resp.status().is_success() {
        debug!(status = %resp.status(), "inference sidecar /explain non-2xx");
        return None;
    }
    match resp.json::<ExplainResponse>().await {
        Ok(body) => Some(body),
        Err(err) => {
            debug!(%err, "inference sidecar /explain parse failed");
            None
        }
    }
}

/// Compute a reproducible SHA-256 hash of the feature vector actually
/// sent to the sidecar. We canonicalize by walking the JSON into a
/// `BTreeMap<String, BTreeMap<String, ...>>` first so key ordering
/// can't perturb the hash.
pub fn feature_hash(features_by_source: &JsonValue) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    let obj = features_by_source.as_object()?;
    let mut canonical: BTreeMap<String, BTreeMap<String, JsonValue>> = BTreeMap::new();
    for (source, feats) in obj.iter() {
        let Some(inner) = feats.as_object() else {
            continue;
        };
        let mut row: BTreeMap<String, JsonValue> = BTreeMap::new();
        for (k, v) in inner.iter() {
            row.insert(k.clone(), v.clone());
        }
        canonical.insert(source.clone(), row);
    }
    let bytes = serde_json::to_vec(&canonical).ok()?;
    let digest = Sha256::digest(&bytes);
    Some(format!("{:x}", digest))
}
