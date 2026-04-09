//! AltType classifier — maps a confluence direction onto one of the
//! four wave-context buckets (TrendLow / ReactionLow / ReversalHigh /
//! SellingHigh) using the macro trend filter.
//!
//! Two-axis dispatch table (CLAUDE.md #1):
//!
//! | Direction | Macro Trend | AltType        |
//! |-----------|-------------|----------------|
//! | Long      | Up          | TrendLow       |
//! | Long      | Down        | ReactionLow    |
//! | Short     | Up          | ReversalHigh   |
//! | Short     | Down        | SellingHigh    |
//!
//! Macro trend rules:
//! - **Up**: `EMA50 > EMA200` **and** `price > EMA200`
//! - **Down**: `EMA50 < EMA200` **and** `price < EMA200`
//! - **Mixed** (one of the two disagrees): fall back to `EMA50 vs
//!   EMA200` alone — the slower MA wins, never returns "neutral".
//!
//! `Direction::Neutral` produces `None` — no setup, nothing to label.
//!
//! Pure function: no DB, no I/O, no allocation. The worker loop
//! computes EMAs from `qtss_market_bars` and calls this once per
//! candidate.

use crate::types::{AltType, Direction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MacroTrend {
    Up,
    Down,
}

fn macro_trend(ema50: f64, ema200: f64, price: f64) -> MacroTrend {
    let strict_up = ema50 > ema200 && price > ema200;
    let strict_down = ema50 < ema200 && price < ema200;
    if strict_up {
        return MacroTrend::Up;
    }
    if strict_down {
        return MacroTrend::Down;
    }
    // Mixed: slower MA wins.
    if ema50 >= ema200 {
        MacroTrend::Up
    } else {
        MacroTrend::Down
    }
}

/// Classify a candidate setup. Returns `None` for neutral direction
/// — those should never reach this function but the guard is cheap.
pub fn classify_alt_type(
    direction: Direction,
    ema50: f64,
    ema200: f64,
    price: f64,
) -> Option<AltType> {
    let trend = match direction {
        Direction::Neutral => return None,
        _ => macro_trend(ema50, ema200, price),
    };
    Some(match (direction, trend) {
        (Direction::Long, MacroTrend::Up) => AltType::TrendLow,
        (Direction::Long, MacroTrend::Down) => AltType::ReactionLow,
        (Direction::Short, MacroTrend::Up) => AltType::ReversalHigh,
        (Direction::Short, MacroTrend::Down) => AltType::SellingHigh,
        // Direction::Neutral filtered above; unreachable preserved
        // for the exhaustiveness check.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_in_uptrend_is_trend_low() {
        // EMA50 > EMA200, price above both
        assert_eq!(
            classify_alt_type(Direction::Long, 110.0, 100.0, 112.0),
            Some(AltType::TrendLow)
        );
    }

    #[test]
    fn long_in_downtrend_is_reaction_low() {
        // EMA50 < EMA200, price below both
        assert_eq!(
            classify_alt_type(Direction::Long, 90.0, 100.0, 88.0),
            Some(AltType::ReactionLow)
        );
    }

    #[test]
    fn short_in_uptrend_is_reversal_high() {
        assert_eq!(
            classify_alt_type(Direction::Short, 110.0, 100.0, 112.0),
            Some(AltType::ReversalHigh)
        );
    }

    #[test]
    fn short_in_downtrend_is_selling_high() {
        assert_eq!(
            classify_alt_type(Direction::Short, 90.0, 100.0, 88.0),
            Some(AltType::SellingHigh)
        );
    }

    #[test]
    fn mixed_ema_above_price_below_falls_back_to_ema_compare() {
        // EMA50>EMA200 but price below EMA200 → mixed → slow MA wins → Up
        assert_eq!(
            classify_alt_type(Direction::Long, 105.0, 100.0, 95.0),
            Some(AltType::TrendLow)
        );
    }

    #[test]
    fn mixed_ema_below_price_above_falls_back_to_ema_compare() {
        // EMA50<EMA200 but price above → slow MA wins → Down
        assert_eq!(
            classify_alt_type(Direction::Short, 95.0, 100.0, 102.0),
            Some(AltType::SellingHigh)
        );
    }

    #[test]
    fn neutral_direction_returns_none() {
        assert_eq!(
            classify_alt_type(Direction::Neutral, 110.0, 100.0, 112.0),
            None
        );
    }

    #[test]
    fn ema_equality_treated_as_up() {
        assert_eq!(
            classify_alt_type(Direction::Long, 100.0, 100.0, 100.0),
            Some(AltType::TrendLow)
        );
    }
}
