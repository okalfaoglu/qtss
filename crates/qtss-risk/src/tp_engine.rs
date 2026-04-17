//! Faz 9.8.0 — TP ladder engine skeleton.
//!
//! Evaluates whether the last mark has crossed any unfilled TP leg and
//! emits the legs to close. Full partial-fill accounting + trailing-TP
//! lands in Faz 9.8.7.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::live_position_store::{LivePositionState, PositionSide};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TpTrigger {
    pub leg_index: usize,
    pub price: Decimal,
    pub qty: Decimal,
}

/// Pure evaluator. Returns the list of TP legs the current mark has
/// crossed and which still have remaining (filled < qty) quantity.
pub fn evaluate(state: &LivePositionState) -> Vec<TpTrigger> {
    let Some(mark) = state.last_mark else {
        return Vec::new();
    };
    state
        .tp_ladder
        .iter()
        .enumerate()
        .filter_map(|(i, leg)| {
            let remaining = leg.qty - leg.filled_qty;
            if remaining <= Decimal::ZERO {
                return None;
            }
            let crossed = match state.side {
                PositionSide::Buy => mark >= leg.price,
                PositionSide::Sell => mark <= leg.price,
            };
            crossed.then_some(TpTrigger {
                leg_index: i,
                price: leg.price,
                qty: remaining,
            })
        })
        .collect()
}
