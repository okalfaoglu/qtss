//! Account / portfolio snapshot used by the risk engine. The risk
//! engine never queries a database directly — callers (qtss-portfolio,
//! qtss-execution) build this snapshot and hand it in. Keeps the crate
//! pure and easy to unit-test.

use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct AccountState {
    /// Current equity in quote currency.
    pub equity: Decimal,
    /// Peak equity reached this session — used to compute drawdown.
    pub peak_equity: Decimal,
    /// Realised + unrealised pnl since the start of the trading day,
    /// expressed in quote currency. Negative = loss.
    pub day_pnl: Decimal,
    /// Number of currently open positions across all instruments.
    pub open_positions: u32,
    /// Aggregate gross notional / equity. `1.0` = no leverage.
    pub current_leverage: Decimal,
    /// Whether the operator has manually flipped the kill switch on.
    pub kill_switch_manual: bool,
}

impl AccountState {
    /// Drawdown as a positive fraction of peak equity. Returns zero
    /// when peak equity is non-positive (defensive — no DB connection
    /// can give us divide-by-zero).
    pub fn drawdown(&self) -> Decimal {
        if self.peak_equity <= Decimal::ZERO {
            return Decimal::ZERO;
        }
        let dd = (self.peak_equity - self.equity) / self.peak_equity;
        if dd < Decimal::ZERO {
            Decimal::ZERO
        } else {
            dd
        }
    }

    /// Day-loss as a positive fraction of equity. Returns zero when the
    /// day is in profit.
    pub fn day_loss_pct(&self) -> Decimal {
        if self.equity <= Decimal::ZERO || self.day_pnl >= Decimal::ZERO {
            return Decimal::ZERO;
        }
        (-self.day_pnl) / self.equity
    }
}
