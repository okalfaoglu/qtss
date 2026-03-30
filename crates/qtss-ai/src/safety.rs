//! Post-parse limits aligned with risk controls (`QTSS_AI_MAX_POSITION_SIZE_MULT`, kill switch).

use serde_json::Value;

use crate::config::AiEngineConfig;

#[derive(Debug, Clone)]
pub struct SafetyConfig {
    pub max_size_multiplier: f64,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        let max = std::env::var("QTSS_AI_MAX_POSITION_SIZE_MULT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.5_f64);
        Self {
            max_size_multiplier: max.max(0.01),
        }
    }
}

impl SafetyConfig {
    pub fn from_ai_engine_config(cfg: &AiEngineConfig) -> Self {
        let s = Self::default();
        // engine config could cap further in future
        let _ = cfg;
        s
    }
}

fn needs_stop_loss(direction: &str) -> bool {
    matches!(
        direction,
        "strong_buy" | "buy" | "sell" | "strong_sell"
    )
}

/// Validates parsed tactical JSON before persistence.
pub fn validate_ai_decision_safety(decision: &Value, config: &SafetyConfig) -> Result<(), &'static str> {
    if qtss_common::is_trading_halted() {
        return Err("trading halted (kill switch)");
    }
    let direction = decision
        .get("direction")
        .and_then(|x| x.as_str())
        .ok_or("missing direction")?;
    let mult = decision
        .get("position_size_multiplier")
        .and_then(|x| x.as_f64())
        .unwrap_or(1.0);
    if mult > config.max_size_multiplier {
        return Err("position_size_multiplier exceeds max_size_multiplier");
    }
    if needs_stop_loss(direction) {
        let sl = decision.get("stop_loss_pct").and_then(|x| x.as_f64());
        if sl.is_none() || sl.unwrap_or(0.0) <= 0.0 {
            return Err("stop_loss_pct required and must be > 0 for directional trades");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn multiplier_cap() {
        let cfg = SafetyConfig {
            max_size_multiplier: 1.2,
        };
        let v = json!({"direction": "neutral", "confidence": 0.5, "position_size_multiplier": 2.0});
        assert!(validate_ai_decision_safety(&v, &cfg).is_err());
    }
}
