//! `/v2/strategies` wire types -- Faz 5 Adim (h).
//!
//! The Strategy Manager card lists every strategy registered with the
//! v2 worker, its enable/pause status, the parameters that drive it,
//! and lightweight runtime counters (signals seen, last intent fired).
//! The DTOs deliberately model parameters as a flat vector of typed
//! key/value pairs so the React form can render an "Edit" sheet
//! without having to know the struct shape of every concrete strategy.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Lifecycle status the operator can flip from the GUI. Mapped 1-1 to
/// the worker's runtime state machine (the worker honours these via
/// the registry handle the route also reads from).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyStatus {
    /// Worker is forwarding signals to this strategy.
    Active,
    /// Strategy is registered but the worker is skipping it (kept hot
    /// so resuming is instant).
    Paused,
    /// Strategy is registered as a definition but never wired to the
    /// signal bus -- e.g. legacy variant kept for audit.
    Disabled,
}

/// One typed parameter exposed in the GUI form. The `kind` discriminant
/// lets the renderer pick a number/string/bool input without parsing
/// the value first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StrategyParam {
    Number { key: String, value: f64 },
    Integer { key: String, value: i64 },
    Bool { key: String, value: bool },
    Text { key: String, value: String },
}

impl StrategyParam {
    pub fn key(&self) -> &str {
        match self {
            StrategyParam::Number { key, .. }
            | StrategyParam::Integer { key, .. }
            | StrategyParam::Bool { key, .. }
            | StrategyParam::Text { key, .. } => key,
        }
    }
}

/// One row in the Strategy Manager card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyCard {
    /// Stable id matching `StrategyProvider::id()`.
    pub id: String,
    /// Human label rendered in the table header.
    pub label: String,
    /// Tag identifying the underlying provider implementation
    /// ("confidence_threshold", "whale_momentum", ...). Used by the
    /// frontend to route the param editor to the right schema.
    pub evaluator: String,
    pub status: StrategyStatus,
    pub params: Vec<StrategyParam>,
    /// Cumulative count of signals dispatched to this strategy
    /// since the worker started.
    pub signals_seen: u64,
    /// Cumulative count of trade intents emitted.
    pub intents_emitted: u64,
    pub last_signal_at: Option<DateTime<Utc>>,
    pub last_intent_at: Option<DateTime<Utc>>,
}

/// Whole `/v2/strategies` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyManagerView {
    pub generated_at: DateTime<Utc>,
    pub strategies: Vec<StrategyCard>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_card() -> StrategyCard {
        StrategyCard {
            id: "trend-1".into(),
            label: "Trend Follower".into(),
            evaluator: "confidence_threshold".into(),
            status: StrategyStatus::Active,
            params: vec![
                StrategyParam::Number { key: "min_confidence".into(), value: 0.7 },
                StrategyParam::Number { key: "risk_pct".into(), value: 0.005 },
                StrategyParam::Bool { key: "act_on_forming".into(), value: false },
            ],
            signals_seen: 12,
            intents_emitted: 3,
            last_signal_at: Some(Utc::now()),
            last_intent_at: Some(Utc::now()),
        }
    }

    #[test]
    fn status_round_trip_uses_snake_case() {
        let j = serde_json::to_string(&StrategyStatus::Paused).unwrap();
        assert_eq!(j, "\"paused\"");
        let back: StrategyStatus = serde_json::from_str("\"disabled\"").unwrap();
        assert_eq!(back, StrategyStatus::Disabled);
    }

    #[test]
    fn param_kind_is_tagged() {
        let p = StrategyParam::Bool { key: "act_on_forming".into(), value: true };
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains("\"kind\":\"bool\""));
    }

    #[test]
    fn param_key_accessor_uniform() {
        assert_eq!(
            StrategyParam::Integer { key: "lookback".into(), value: 50 }.key(),
            "lookback"
        );
        assert_eq!(
            StrategyParam::Text { key: "regime".into(), value: "trend".into() }.key(),
            "regime"
        );
    }

    #[test]
    fn json_round_trip() {
        let view = StrategyManagerView {
            generated_at: Utc::now(),
            strategies: vec![sample_card()],
        };
        let j = serde_json::to_string(&view).unwrap();
        let back: StrategyManagerView = serde_json::from_str(&j).unwrap();
        assert_eq!(back.strategies.len(), 1);
        assert_eq!(back.strategies[0].params.len(), 3);
    }
}
