//! Faz 9.8.7 — TP ladder engine with partial-fill + trailing support.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::live_position_store::{LivePositionState, PositionSide};
#[cfg(test)]
use crate::live_position_store::TpLeg;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TpTrigger {
    pub leg_index: usize,
    pub price: Decimal,
    pub qty: Decimal,
}

pub fn evaluate(state: &LivePositionState) -> Vec<TpTrigger> {
    let Some(mark) = state.last_mark else { return Vec::new() };
    state
        .tp_ladder
        .iter()
        .enumerate()
        .filter_map(|(i, leg)| {
            let remaining = leg.qty - leg.filled_qty;
            if remaining <= Decimal::ZERO { return None; }
            let crossed = match state.side {
                PositionSide::Buy => mark >= leg.price,
                PositionSide::Sell => mark <= leg.price,
            };
            crossed.then_some(TpTrigger { leg_index: i, price: leg.price, qty: remaining })
        })
        .collect()
}

/// Apply a partial fill; clip to remaining, return absorbed qty, shrink
/// position's outstanding size.
pub fn record_partial_fill(state: &mut LivePositionState, leg_index: usize, qty: Decimal) -> Decimal {
    let Some(leg) = state.tp_ladder.get_mut(leg_index) else { return Decimal::ZERO };
    let remaining = leg.qty - leg.filled_qty;
    if remaining <= Decimal::ZERO || qty <= Decimal::ZERO { return Decimal::ZERO; }
    let absorbed = if qty > remaining { remaining } else { qty };
    leg.filled_qty += absorbed;
    if state.qty_remaining >= absorbed {
        state.qty_remaining -= absorbed;
    } else {
        state.qty_remaining = Decimal::ZERO;
    }
    absorbed
}

/// Extend the last TP leg by `atr * mult` when the mark has passed it.
pub fn trail_last_leg(state: &mut LivePositionState, atr: Decimal, mult: Decimal) -> Option<Decimal> {
    if atr <= Decimal::ZERO || mult <= Decimal::ZERO { return None; }
    let mark = state.last_mark?;
    let leg = state.tp_ladder.last_mut()?;
    let should_trail = match state.side {
        PositionSide::Buy => mark >= leg.price,
        PositionSide::Sell => mark <= leg.price,
    };
    if !should_trail { return None; }
    let dist = atr * mult;
    let new_price = match state.side {
        PositionSide::Buy => mark + dist,
        PositionSide::Sell => mark - dist,
    };
    leg.price = new_price;
    Some(new_price)
}

pub fn remaining_qty(state: &LivePositionState) -> Decimal {
    state.tp_ladder.iter().fold(Decimal::ZERO, |acc, leg| {
        let r = leg.qty - leg.filled_qty;
        acc + if r > Decimal::ZERO { r } else { Decimal::ZERO }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_position_store::{ExecutionMode, MarketSegment};
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn long_state(mark: Decimal, ladder: Vec<TpLeg>) -> LivePositionState {
        LivePositionState {
            id: Uuid::new_v4(),
            setup_id: None,
            mode: ExecutionMode::Dry,
            exchange: "binance".into(),
            segment: MarketSegment::Futures,
            symbol: "BTCUSDT".into(),
            side: PositionSide::Buy,
            leverage: 10,
            entry_avg: dec!(100),
            qty_filled: dec!(1),
            qty_remaining: dec!(1),
            current_sl: Some(dec!(98)),
            tp_ladder: ladder,
            liquidation_price: None,
            maint_margin_ratio: None,
            funding_rate_next: None,
            last_mark: Some(mark),
            last_tick_at: Some(Utc::now()),
            opened_at: Utc::now(),
        }
    }

    #[test]
    fn evaluate_skips_filled_and_uncrossed() {
        let s = long_state(dec!(107), vec![
            TpLeg { price: dec!(105), qty: dec!(0.5), filled_qty: dec!(0) },
            TpLeg { price: dec!(106), qty: dec!(0.3), filled_qty: dec!(0.3) },
            TpLeg { price: dec!(110), qty: dec!(0.2), filled_qty: dec!(0) },
        ]);
        let t = evaluate(&s);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].leg_index, 0);
    }

    #[test]
    fn record_partial_fill_clips_and_shrinks_qty() {
        let mut s = long_state(dec!(107), vec![
            TpLeg { price: dec!(105), qty: dec!(0.5), filled_qty: dec!(0.3) },
        ]);
        let a = record_partial_fill(&mut s, 0, dec!(1.0));
        assert_eq!(a, dec!(0.2));
        assert_eq!(s.qty_remaining, dec!(0.8));
    }

    #[test]
    fn record_partial_fill_noop_on_full_leg() {
        let mut s = long_state(dec!(107), vec![
            TpLeg { price: dec!(105), qty: dec!(0.5), filled_qty: dec!(0.5) },
        ]);
        assert_eq!(record_partial_fill(&mut s, 0, dec!(0.1)), Decimal::ZERO);
    }

    #[test]
    fn trail_last_leg_moves_on_cross() {
        let mut s = long_state(dec!(120), vec![
            TpLeg { price: dec!(110), qty: dec!(0.5), filled_qty: dec!(0) },
        ]);
        assert_eq!(trail_last_leg(&mut s, dec!(2), dec!(2)), Some(dec!(124)));
    }

    #[test]
    fn trail_last_leg_noop_before_cross() {
        let mut s = long_state(dec!(105), vec![
            TpLeg { price: dec!(110), qty: dec!(0.5), filled_qty: dec!(0) },
        ]);
        assert!(trail_last_leg(&mut s, dec!(2), dec!(2)).is_none());
    }

    #[test]
    fn remaining_qty_sums_unfilled_only() {
        let s = long_state(dec!(100), vec![
            TpLeg { price: dec!(105), qty: dec!(0.5), filled_qty: dec!(0.2) },
            TpLeg { price: dec!(110), qty: dec!(0.3), filled_qty: dec!(0) },
        ]);
        assert_eq!(remaining_qty(&s), dec!(0.6));
    }

    #[test]
    fn short_side_cross_inverts() {
        let mut s = long_state(dec!(95), vec![
            TpLeg { price: dec!(96), qty: dec!(0.5), filled_qty: dec!(0) },
        ]);
        s.side = PositionSide::Sell;
        assert_eq!(evaluate(&s).len(), 1);
    }
}
