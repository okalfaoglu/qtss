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

const RISKY_OPERATIONAL_ACTIONS: &[&str] = &[
    "full_close",
    "add_to_position",
    "widen_stop",
    "deactivate_trailing",
];

/// Validates parsed operational directive JSON before persistence.
pub fn validate_operational_decision_safety(decision: &Value, _config: &SafetyConfig) -> Result<(), &'static str> {
    if qtss_common::is_trading_halted() {
        return Err("trading halted (kill switch)");
    }
    let action = decision
        .get("action")
        .and_then(|x| x.as_str())
        .ok_or("missing action")?;
    if RISKY_OPERATIONAL_ACTIONS.contains(&action) {
        let reasoning = decision.get("reasoning").and_then(|x| x.as_str()).unwrap_or("");
        if reasoning.trim().is_empty() {
            return Err("risky operational action requires reasoning");
        }
    }
    if action == "partial_close" {
        let pct = decision.get("partial_close_pct").and_then(|x| x.as_f64());
        match pct {
            Some(p) if p > 0.0 && p <= 100.0 => {}
            Some(_) => return Err("partial_close_pct must be > 0 and <= 100"),
            None => return Err("partial_close requires partial_close_pct"),
        }
    }
    if action == "activate_trailing" {
        let cb = decision.get("trailing_callback_pct").and_then(|x| x.as_f64());
        if cb.is_none() || cb.unwrap_or(0.0) <= 0.0 {
            return Err("activate_trailing requires positive trailing_callback_pct");
        }
    }
    Ok(())
}

/// Validates parsed strategic/portfolio directive JSON before persistence.
pub fn validate_strategic_decision_safety(decision: &Value, _config: &SafetyConfig) -> Result<(), &'static str> {
    if qtss_common::is_trading_halted() {
        return Err("trading halted (kill switch)");
    }
    if let Some(rbp) = decision.get("risk_budget_pct").and_then(|x| x.as_f64()) {
        if rbp < 0.0 || rbp > 100.0 {
            return Err("risk_budget_pct must be 0..=100");
        }
    }
    if let Some(mop) = decision.get("max_open_positions").and_then(|x| x.as_i64()) {
        if mop < 0 || mop > 200 {
            return Err("max_open_positions must be 0..=200");
        }
    }
    if let Some(scores) = decision.get("symbol_scores").and_then(|x| x.as_object()) {
        for (sym, w) in scores {
            if let Some(v) = w.as_f64() {
                if v < 0.0 || v > 1.0 {
                    return Err("symbol_scores weights must be 0.0..=1.0");
                }
            }
            if sym.trim().is_empty() {
                return Err("symbol_scores contains empty symbol key");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    static SAFETY_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn multiplier_cap() {
        let cfg = SafetyConfig {
            max_size_multiplier: 1.2,
        };
        let v = json!({"direction": "neutral", "confidence": 0.5, "position_size_multiplier": 2.0});
        assert!(validate_ai_decision_safety(&v, &cfg).is_err());
    }

    #[test]
    fn directional_trade_requires_positive_stop_loss() {
        let _g = SAFETY_TEST_LOCK.lock().expect("safety test lock");
        qtss_common::clear_trading_halt();
        let cfg = SafetyConfig {
            max_size_multiplier: 2.0,
        };
        let missing_sl = json!({"direction": "buy", "confidence": 0.8});
        assert!(validate_ai_decision_safety(&missing_sl, &cfg).is_err());
        let zero_sl = json!({"direction": "buy", "confidence": 0.8, "stop_loss_pct": 0.0});
        assert!(validate_ai_decision_safety(&zero_sl, &cfg).is_err());
        let ok = json!({"direction": "buy", "confidence": 0.8, "stop_loss_pct": 1.5});
        assert!(validate_ai_decision_safety(&ok, &cfg).is_ok());
    }

    #[test]
    fn neutral_skips_stop_loss_requirement() {
        let _g = SAFETY_TEST_LOCK.lock().expect("safety test lock");
        qtss_common::clear_trading_halt();
        let cfg = SafetyConfig {
            max_size_multiplier: 2.0,
        };
        let v = json!({"direction": "neutral", "confidence": 0.5});
        assert!(validate_ai_decision_safety(&v, &cfg).is_ok());
    }

    #[test]
    fn rejects_when_trading_halted() {
        let _g = SAFETY_TEST_LOCK.lock().expect("safety test lock");
        qtss_common::set_trading_halted(true);
        let cfg = SafetyConfig {
            max_size_multiplier: 2.0,
        };
        let v = json!({"direction": "neutral", "confidence": 0.5});
        assert_eq!(
            validate_ai_decision_safety(&v, &cfg),
            Err("trading halted (kill switch)")
        );
        qtss_common::clear_trading_halt();
    }
}
