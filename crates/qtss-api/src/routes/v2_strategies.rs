#![allow(dead_code)]
//! `GET /v2/strategies` -- Faz 5 Adim (h).
//!
//! In-memory strategy registry handle, mirrored on `SharedState`. The
//! v2 worker (Faz 6) will register every `StrategyProvider` it spins
//! up here, and call `record_signal` / `record_intent` as it dispatches
//! traffic. Today the registry is seeded with one default
//! `confidence_threshold` card so the GUI has something concrete to
//! render against the new wire shape -- the parameters come from
//! `system_config` (env fallback) so nothing on this route is
//! hardcoded (CLAUDE.md #2).

use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use qtss_gui_api::{
    StrategyCard, StrategyManagerView, StrategyParam, StrategyStatus,
};

use crate::error::ApiError;
use crate::state::SharedState;

/// Shared registry slot exposed via `SharedState`.
pub struct V2StrategyRegistry {
    inner: RwLock<Vec<StrategyCard>>,
}

impl V2StrategyRegistry {
    pub fn new(seed: Vec<StrategyCard>) -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(seed),
        })
    }

    /// Snapshot the current registry into the wire DTO.
    pub async fn snapshot(&self) -> StrategyManagerView {
        let g = self.inner.read().await;
        StrategyManagerView {
            generated_at: Utc::now(),
            strategies: g.clone(),
        }
    }

    /// Register (or replace) a strategy card. The worker will call
    /// this every time it constructs a `StrategyProvider`.
    pub async fn upsert(&self, card: StrategyCard) {
        let mut g = self.inner.write().await;
        if let Some(slot) = g.iter_mut().find(|c| c.id == card.id) {
            *slot = card;
        } else {
            g.push(card);
        }
    }

    /// Bump the `signals_seen` counter and timestamp.
    pub async fn record_signal(&self, id: &str, at: DateTime<Utc>) {
        let mut g = self.inner.write().await;
        if let Some(c) = g.iter_mut().find(|c| c.id == id) {
            c.signals_seen = c.signals_seen.saturating_add(1);
            c.last_signal_at = Some(at);
        }
    }

    /// Bump the `intents_emitted` counter and timestamp.
    pub async fn record_intent(&self, id: &str, at: DateTime<Utc>) {
        let mut g = self.inner.write().await;
        if let Some(c) = g.iter_mut().find(|c| c.id == id) {
            c.intents_emitted = c.intents_emitted.saturating_add(1);
            c.last_intent_at = Some(at);
        }
    }

    /// Flip status. The worker honours this on the next dispatch tick.
    pub async fn set_status(&self, id: &str, status: StrategyStatus) -> bool {
        let mut g = self.inner.write().await;
        if let Some(c) = g.iter_mut().find(|c| c.id == id) {
            c.status = status;
            return true;
        }
        false
    }
}

/// Build the default seeded card from env-resolved params. Used by
/// `AppState::new` when the runtime registry is empty so the GUI sees
/// at least one entry on a fresh deployment.
pub fn default_seed_card() -> StrategyCard {
    let min_confidence: f64 = env_f64("QTSS_V2_STRAT_MIN_CONFIDENCE", 0.7);
    let risk_pct: f64 = env_f64("QTSS_V2_STRAT_RISK_PCT", 0.005);
    let act_on_forming: bool = env_bool("QTSS_V2_STRAT_ACT_ON_FORMING", false);
    let time_stop_secs: i64 = env_int("QTSS_V2_STRAT_TIME_STOP_SECS", 3600);

    StrategyCard {
        id: "confidence_threshold_default".into(),
        label: "Confidence Threshold (default)".into(),
        evaluator: "confidence_threshold".into(),
        status: StrategyStatus::Active,
        params: vec![
            StrategyParam::Number { key: "min_confidence".into(), value: min_confidence },
            StrategyParam::Number { key: "risk_pct".into(), value: risk_pct },
            StrategyParam::Bool { key: "act_on_forming".into(), value: act_on_forming },
            StrategyParam::Integer { key: "time_stop_secs".into(), value: time_stop_secs },
        ],
        signals_seen: 0,
        intents_emitted: 0,
        last_signal_at: None,
        last_intent_at: None,
    }
}

pub fn v2_strategies_router() -> Router<SharedState> {
    Router::new().route("/v2/strategies", get(get_strategies))
}

async fn get_strategies(
    State(st): State<SharedState>,
) -> Result<Json<StrategyManagerView>, ApiError> {
    Ok(Json(st.v2_strategies.snapshot().await))
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn env_int(key: &str, default: i64) -> i64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_replaces_existing() {
        let reg = V2StrategyRegistry::new(vec![default_seed_card()]);
        let mut updated = default_seed_card();
        updated.label = "renamed".into();
        reg.upsert(updated).await;
        let snap = reg.snapshot().await;
        assert_eq!(snap.strategies.len(), 1);
        assert_eq!(snap.strategies[0].label, "renamed");
    }

    #[tokio::test]
    async fn record_signal_and_intent_increment() {
        let reg = V2StrategyRegistry::new(vec![default_seed_card()]);
        let id = "confidence_threshold_default";
        reg.record_signal(id, Utc::now()).await;
        reg.record_signal(id, Utc::now()).await;
        reg.record_intent(id, Utc::now()).await;
        let snap = reg.snapshot().await;
        assert_eq!(snap.strategies[0].signals_seen, 2);
        assert_eq!(snap.strategies[0].intents_emitted, 1);
        assert!(snap.strategies[0].last_signal_at.is_some());
    }

    #[tokio::test]
    async fn set_status_returns_false_for_unknown() {
        let reg = V2StrategyRegistry::new(vec![default_seed_card()]);
        assert!(!reg.set_status("nope", StrategyStatus::Paused).await);
        assert!(reg.set_status("confidence_threshold_default", StrategyStatus::Paused).await);
        let snap = reg.snapshot().await;
        assert_eq!(snap.strategies[0].status, StrategyStatus::Paused);
    }
}
