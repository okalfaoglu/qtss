//! Faz 9.8.0 — Liquidation guard skeleton.
//!
//! Evaluates the distance between `last_mark` and `liquidation_price`
//! for a live position and emits a `LiquidationAction`. The full body
//! (auto add-margin + panic close + event persistence) is filled in
//! Faz 9.8.3. Here we fix the trait surface so 9.8.5's tick fan-out
//! can compile against it.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::live_position_store::LivePositionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiquidationSeverity {
    /// No action — distance is comfortable.
    Safe,
    /// First alert threshold (default 8%).
    Warn,
    /// Auto-action threshold (default 4%) — add margin / scale out.
    Critical,
    /// Panic-close threshold (default 1.5%) — market out now.
    Breach,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiquidationAction {
    None,
    Alert,
    AddMargin,
    ScaleOut,
    PanicClose,
}

#[derive(Debug, Clone)]
pub struct LiquidationGuardConfig {
    pub warn_distance_pct: Decimal,
    pub critical_distance_pct: Decimal,
    pub panic_close_distance_pct: Decimal,
}

impl Default for LiquidationGuardConfig {
    fn default() -> Self {
        Self {
            warn_distance_pct: Decimal::new(8, 2),     // 0.08
            critical_distance_pct: Decimal::new(4, 2), // 0.04
            panic_close_distance_pct: Decimal::new(15, 3), // 0.015
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiquidationAssessment {
    pub severity: LiquidationSeverity,
    pub action: LiquidationAction,
    pub mark: Decimal,
    pub liquidation: Decimal,
    pub distance_pct: Decimal,
}

/// Pure evaluator — no I/O. Caller persists outcome to
/// `liquidation_guard_events` when severity != Safe.
pub fn assess(
    state: &LivePositionState,
    cfg: &LiquidationGuardConfig,
) -> Option<LiquidationAssessment> {
    let mark = state.last_mark?;
    let liq = state.liquidation_price?;
    if mark <= Decimal::ZERO || liq <= Decimal::ZERO {
        return None;
    }
    // Distance is always reported as a positive fraction — the side
    // determines which direction "closer to liquidation" means, but
    // the magnitude alone drives the severity ladder.
    let diff = if liq > mark { liq - mark } else { mark - liq };
    let distance_pct = diff / mark;

    let severity = if distance_pct <= cfg.panic_close_distance_pct {
        LiquidationSeverity::Breach
    } else if distance_pct <= cfg.critical_distance_pct {
        LiquidationSeverity::Critical
    } else if distance_pct <= cfg.warn_distance_pct {
        LiquidationSeverity::Warn
    } else {
        LiquidationSeverity::Safe
    };

    let action = match severity {
        LiquidationSeverity::Safe => LiquidationAction::None,
        LiquidationSeverity::Warn => LiquidationAction::Alert,
        // 9.8.3 will choose between AddMargin and ScaleOut based on
        // margin availability; placeholder picks ScaleOut as the safer
        // default (no extra capital commitment).
        LiquidationSeverity::Critical => LiquidationAction::ScaleOut,
        LiquidationSeverity::Breach => LiquidationAction::PanicClose,
    };

    Some(LiquidationAssessment {
        severity,
        action,
        mark,
        liquidation: liq,
        distance_pct,
    })
}
