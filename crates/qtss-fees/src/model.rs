//! `FeeModel` trait — the boundary execution adapters depend on.

use crate::error::FeeResult;
use crate::schedule::Liquidity;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Inputs needed to price a fill. Kept tiny on purpose — anything
/// else (tiered VIP discounts, BNB rebates, …) is encoded inside the
/// concrete model implementation, not pushed onto callers.
#[derive(Debug, Clone)]
pub struct TradeContext<'a> {
    pub venue: &'a str,
    pub symbol: &'a str,
    pub price: Decimal,
    pub quantity: Decimal,
    pub liquidity: Liquidity,
}

impl<'a> TradeContext<'a> {
    pub fn notional(&self) -> Decimal {
        self.price * self.quantity
    }
}

/// The fee charge produced for a single fill. Split into components
/// so the portfolio engine and reporting layer can attribute costs
/// without re-deriving them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeeQuote {
    /// Total fee in quote currency. Always positive (a cost).
    pub total: Decimal,
    /// Component charged at the venue's percentage rate.
    pub percentage_component: Decimal,
    /// Flat per-trade component (regulatory/exchange).
    pub flat_component: Decimal,
    /// Effective rate (`total / notional`) — handy for analytics.
    pub effective_rate: Decimal,
}

pub trait FeeModel: Send + Sync {
    fn quote(&self, ctx: &TradeContext<'_>) -> FeeResult<FeeQuote>;
}
