//! Faz 9.8.0 — SL ratchet skeleton.
//!
//! Moves the stop-loss in the favourable direction only (never
//! loosens). Full policies (breakeven-on-R1, trailing-by-ATR, chandelier,
//! etc.) land in Faz 9.8.7 via a dispatch table. This skeleton exposes
//! the decision surface.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::live_position_store::{LivePositionState, PositionSide};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RatchetKind {
    None,
    Breakeven,
    Trailing,
    Chandelier,
}

#[derive(Debug, Clone)]
pub struct RatchetDecision {
    pub kind: RatchetKind,
    pub new_sl: Option<Decimal>,
}

impl RatchetDecision {
    pub fn none() -> Self {
        Self {
            kind: RatchetKind::None,
            new_sl: None,
        }
    }
}

/// Safety: the returned SL must only tighten (move towards mark) —
/// never widen. Caller enforces the invariant before committing.
pub fn tighten_only(state: &LivePositionState, new_sl: Decimal) -> Option<Decimal> {
    let current = state.current_sl?;
    let tighter = match state.side {
        PositionSide::Buy => new_sl > current,
        PositionSide::Sell => new_sl < current,
    };
    tighter.then_some(new_sl)
}

pub fn evaluate(_state: &LivePositionState) -> RatchetDecision {
    RatchetDecision::none()
}
