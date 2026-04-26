//! Cost model — fees, slippage, and (optionally) funding.
//!
//! Mirrors the live execution path approximately enough for the
//! backtest's PnL to be a useful upper-bound estimate without
//! over-engineering. Live reality is messier (queue position, partial
//! fills, IOC rejection, etc.) — when the backtest decides a trade
//! is winning by < ~0.1%, treat that as a wash, not a real edge.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Per-fill cost components — itemised so attribution can see WHICH
/// cost dominated for losing trades (slippage on entry vs funding
/// drag over the holding window vs taker fee on every level).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FillCost {
    /// Maker / taker fee in quote currency (e.g. USDT).
    pub fee: Decimal,
    /// Slippage applied as a price displacement vs the bar close,
    /// expressed in quote currency for the position size.
    pub slippage: Decimal,
    /// Funding paid (or received) over the holding window. Positive
    /// = paid, negative = received.
    pub funding: Decimal,
}

impl FillCost {
    pub fn zero() -> Self {
        Self {
            fee: Decimal::ZERO,
            slippage: Decimal::ZERO,
            funding: Decimal::ZERO,
        }
    }

    pub fn total(&self) -> Decimal {
        self.fee + self.slippage + self.funding
    }
}

/// Cost model parameters. Defaults are tuned for Binance USDT-M
/// perpetuals (taker 0.04%, average slippage ~0.02%, average funding
/// ±0.01% per 8h window). Pass venue-specific overrides via the
/// backtest config when running other markets.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CostModel {
    /// Taker fee rate (decimal, e.g. 0.0004 = 4 bps).
    pub taker_fee_rate: Decimal,
    /// Maker fee rate. v1 always uses taker; maker is parked here for
    /// later when the runner gains a "post-only" entry mode.
    pub maker_fee_rate: Decimal,
    /// Avg slippage as a fraction of price (e.g. 0.0002 = 2 bps).
    pub slippage_rate: Decimal,
    /// Funding rate per 8h window (decimal). Positive = longs pay
    /// shorts. v1 applies it linearly to the holding duration.
    pub funding_rate_per_8h: Decimal,
}

impl Default for CostModel {
    fn default() -> Self {
        // Binance USDT-M perp v1 defaults (live taker = 4 bps,
        // typical observed slippage 1-3 bps, avg funding ±1 bp per
        // 8h on majors).
        Self {
            taker_fee_rate: Decimal::new(4, 4),    // 0.0004
            maker_fee_rate: Decimal::new(2, 4),    // 0.0002
            slippage_rate: Decimal::new(2, 4),     // 0.0002
            funding_rate_per_8h: Decimal::new(1, 4), // 0.0001
        }
    }
}

impl CostModel {
    /// Compute the cost of a single fill. `notional` is
    /// `qty * price`. Funding is charged proportional to
    /// `holding_secs` divided by the 8-hour window (28800s).
    ///
    /// Sign convention: `is_long_taker_perp` adjusts the funding
    /// direction — a long perp pays a positive funding rate, a short
    /// perp receives it. v1 applies the abs of the rate uniformly
    /// (charge longs, credit shorts). `is_long` is_long_perp toggles
    /// the credit direction.
    pub fn fill_cost(
        &self,
        notional: Decimal,
        holding_secs: i64,
        is_long: bool,
    ) -> FillCost {
        let abs_notional = notional.abs();
        let fee = abs_notional * self.taker_fee_rate;
        let slippage = abs_notional * self.slippage_rate;
        // Funding: holding_secs/28800 * rate * notional. Long pays if
        // rate > 0, short receives; flip sign for shorts. Backtest
        // assumes `funding_rate_per_8h` is the average over the
        // holding window (a reasonable approximation; per-tick
        // funding history loader is FAZ 26.4).
        let secs_per_window = Decimal::new(28_800, 0);
        let windows = if secs_per_window.is_zero() {
            Decimal::ZERO
        } else {
            Decimal::from(holding_secs.max(0)) / secs_per_window
        };
        let funding_signed = if is_long {
            abs_notional * self.funding_rate_per_8h * windows
        } else {
            -(abs_notional * self.funding_rate_per_8h * windows)
        };
        FillCost {
            fee,
            slippage,
            funding: funding_signed,
        }
    }

    /// Convenience — total cost as a fraction of notional, used by
    /// the commission gate to skip low-edge setups.
    pub fn round_trip_cost_fraction(&self, holding_secs: i64) -> f64 {
        // Two fills (entry + exit) + slippage on both + one-way funding.
        let fee = self.taker_fee_rate.to_f64().unwrap_or(0.0) * 2.0;
        let slip = self.slippage_rate.to_f64().unwrap_or(0.0) * 2.0;
        let secs_per_window = 28_800.0_f64;
        let windows = (holding_secs.max(0) as f64) / secs_per_window;
        let funding =
            self.funding_rate_per_8h.to_f64().unwrap_or(0.0).abs() * windows;
        fee + slip + funding
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn default_round_trip_about_12_bps_one_window() {
        let m = CostModel::default();
        // 8h hold: 2*4 (fees) + 2*2 (slip) + 1 (funding) ≈ 13 bps.
        let cost = m.round_trip_cost_fraction(28_800);
        assert!(cost > 0.0010 && cost < 0.0020, "got {cost}");
    }

    #[test]
    fn long_pays_funding_short_receives() {
        let m = CostModel::default();
        let long = m.fill_cost(dec!(10000), 28_800, true).funding;
        let short = m.fill_cost(dec!(10000), 28_800, false).funding;
        assert!(long > Decimal::ZERO);
        assert!(short < Decimal::ZERO);
    }

    #[test]
    fn zero_holding_zero_funding() {
        let m = CostModel::default();
        let f = m.fill_cost(dec!(10000), 0, true);
        assert_eq!(f.funding, Decimal::ZERO);
        // Fees still apply.
        assert!(f.fee > Decimal::ZERO);
    }
}
