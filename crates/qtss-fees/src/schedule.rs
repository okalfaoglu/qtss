//! Per-venue fee schedule.

use crate::error::{FeeError, FeeResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Whether the trade added or removed liquidity. Drives maker vs taker
/// rate selection without callers touching match arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Liquidity {
    Maker,
    Taker,
}

/// Fee schedule for one (venue, symbol) cell. Rates are fractions of
/// notional (0.001 = 10 bps). `min_fee` is an absolute floor in quote
/// currency — some venues charge a minimum even on tiny clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeeSchedule {
    pub maker_pct: Decimal,
    pub taker_pct: Decimal,
    /// Optional absolute minimum fee in quote currency.
    pub min_fee: Option<Decimal>,
    /// Extra per-trade exchange/regulatory fee in quote currency
    /// (e.g. SEC/FINRA on US equities, BSMV on BIST).
    pub per_trade_flat: Option<Decimal>,
}

impl FeeSchedule {
    pub fn new(maker_pct: Decimal, taker_pct: Decimal) -> FeeResult<Self> {
        Self::with_extras(maker_pct, taker_pct, None, None)
    }

    pub fn with_extras(
        maker_pct: Decimal,
        taker_pct: Decimal,
        min_fee: Option<Decimal>,
        per_trade_flat: Option<Decimal>,
    ) -> FeeResult<Self> {
        if maker_pct < Decimal::ZERO || taker_pct < Decimal::ZERO {
            return Err(FeeError::Invalid("fee rates must be non-negative".into()));
        }
        Ok(Self { maker_pct, taker_pct, min_fee, per_trade_flat })
    }

    /// Look up the rate for a given liquidity side. Single dispatch
    /// point so callers never need to write `match liquidity { … }`.
    pub fn rate(&self, liquidity: Liquidity) -> Decimal {
        match liquidity {
            Liquidity::Maker => self.maker_pct,
            Liquidity::Taker => self.taker_pct,
        }
    }
}
