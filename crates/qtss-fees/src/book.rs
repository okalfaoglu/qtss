//! `FeeBook` — registry of per-(venue, symbol) schedules.
//!
//! Lookup precedence: exact `(venue, symbol)` → venue default
//! (`symbol = "*"`). Anything that misses both is an error: we never
//! silently fall back to a hardcoded rate (CLAUDE.md rule #2).

use crate::error::{FeeError, FeeResult};
use crate::model::{FeeModel, FeeQuote, TradeContext};
use crate::schedule::FeeSchedule;
use rust_decimal::Decimal;
use std::collections::HashMap;

const VENUE_DEFAULT: &str = "*";

#[derive(Debug, Default, Clone)]
pub struct FeeBook {
    /// venue → (symbol|"*") → schedule
    schedules: HashMap<String, HashMap<String, FeeSchedule>>,
}

impl FeeBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the venue-wide default. Used when a symbol has no
    /// override of its own.
    pub fn register_venue_default(&mut self, venue: &str, schedule: FeeSchedule) {
        self.schedules
            .entry(venue.to_string())
            .or_default()
            .insert(VENUE_DEFAULT.to_string(), schedule);
    }

    /// Register a per-symbol override (e.g. BTCUSDT VIP rates).
    pub fn register_symbol(&mut self, venue: &str, symbol: &str, schedule: FeeSchedule) {
        self.schedules
            .entry(venue.to_string())
            .or_default()
            .insert(symbol.to_string(), schedule);
    }

    fn lookup(&self, venue: &str, symbol: &str) -> FeeResult<&FeeSchedule> {
        let venue_map = self
            .schedules
            .get(venue)
            .ok_or_else(|| FeeError::UnknownVenue(venue.to_string()))?;
        venue_map
            .get(symbol)
            .or_else(|| venue_map.get(VENUE_DEFAULT))
            .ok_or_else(|| FeeError::UnknownVenue(format!("{venue}:{symbol}")))
    }
}

impl FeeModel for FeeBook {
    fn quote(&self, ctx: &TradeContext<'_>) -> FeeResult<FeeQuote> {
        let schedule = self.lookup(ctx.venue, ctx.symbol)?;
        let notional = ctx.notional();
        let rate = schedule.rate(ctx.liquidity);
        let mut percentage = notional * rate;
        if let Some(min) = schedule.min_fee {
            if percentage < min {
                percentage = min;
            }
        }
        let flat = schedule.per_trade_flat.unwrap_or(Decimal::ZERO);
        let total = percentage + flat;
        let effective_rate = if notional > Decimal::ZERO {
            total / notional
        } else {
            Decimal::ZERO
        };
        Ok(FeeQuote {
            total,
            percentage_component: percentage,
            flat_component: flat,
            effective_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::Liquidity;
    use rust_decimal_macros::dec;

    fn book() -> FeeBook {
        let mut b = FeeBook::new();
        b.register_venue_default(
            "binance",
            FeeSchedule::new(dec!(0.0002), dec!(0.0005)).unwrap(),
        );
        b.register_symbol(
            "binance",
            "BTCUSDT",
            FeeSchedule::with_extras(
                dec!(0.0001),
                dec!(0.0003),
                Some(dec!(0.10)),
                None,
            )
            .unwrap(),
        );
        b
    }

    #[test]
    fn taker_uses_taker_rate() {
        let q = book()
            .quote(&TradeContext {
                venue: "binance",
                symbol: "ETHUSDT",
                price: dec!(2000),
                quantity: dec!(1),
                liquidity: Liquidity::Taker,
            })
            .unwrap();
        assert_eq!(q.percentage_component, dec!(1.0)); // 2000 * 0.0005
        assert_eq!(q.total, dec!(1.0));
    }

    #[test]
    fn symbol_override_beats_venue_default() {
        let q = book()
            .quote(&TradeContext {
                venue: "binance",
                symbol: "BTCUSDT",
                price: dec!(50000),
                quantity: dec!(0.01),
                liquidity: Liquidity::Maker,
            })
            .unwrap();
        // 500 notional * 0.0001 = 0.05 → bumped to min 0.10
        assert_eq!(q.percentage_component, dec!(0.10));
    }

    #[test]
    fn unknown_venue_errors() {
        let err = book()
            .quote(&TradeContext {
                venue: "kraken",
                symbol: "BTCUSDT",
                price: dec!(1),
                quantity: dec!(1),
                liquidity: Liquidity::Taker,
            })
            .unwrap_err();
        assert!(matches!(err, FeeError::UnknownVenue(_)));
    }
}
