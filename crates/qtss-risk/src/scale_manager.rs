//! Faz 9.8.0 — Scale manager skeleton (pyramid-in / scale-out / add-on-dip).
//!
//! Full decision logic lands in Faz 9.8.6. This skeleton fixes the
//! types so the tick fan-out in 9.8.5 can call into it.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::live_position_store::LivePositionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleDecisionKind {
    Hold,
    PyramidIn,
    ScaleOut,
    AddOnDip,
    PartialTp,
}

#[derive(Debug, Clone)]
pub struct ScaleDecision {
    pub kind: ScaleDecisionKind,
    pub qty_delta: Decimal,
    pub reason: &'static str,
}

impl ScaleDecision {
    pub fn hold() -> Self {
        Self {
            kind: ScaleDecisionKind::Hold,
            qty_delta: Decimal::ZERO,
            reason: "no scale condition met",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScaleManagerConfig {
    pub enabled: bool,
    pub max_pyramid_legs: u8,
}

impl Default for ScaleManagerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_pyramid_legs: 2,
        }
    }
}

/// Pure evaluator. 9.8.6 will plug in real indicators (ATR band,
/// dip depth vs entry, R-multiple progression) — this placeholder
/// always returns `Hold`.
pub fn evaluate(_state: &LivePositionState, _cfg: &ScaleManagerConfig) -> ScaleDecision {
    ScaleDecision::hold()
}
