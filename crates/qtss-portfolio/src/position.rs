//! Position bookkeeping.
//!
//! Positions are net per-instrument: a long fill on top of an existing
//! short reduces the short, then opens a long for the residual. Average
//! entry is recomputed only on additions to an existing direction; a
//! reduction realises pnl on the reduced quantity at the existing
//! average.

use qtss_domain::v2::intent::Side;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    /// Net side. `None` when the position is flat.
    pub side: Option<Side>,
    /// Net quantity (always >= 0). When `side` is `None`, this is `0`.
    pub quantity: Decimal,
    /// Average entry price for the *current* open quantity.
    pub avg_entry: Decimal,
    /// Cumulative realised pnl for this instrument since session start.
    pub realised_pnl: Decimal,
    /// Mark-to-market unrealised pnl, refreshed by [`Position::mark`].
    pub unrealised_pnl: Decimal,
    /// Last mark price the engine pushed in (for explainability).
    pub last_mark: Decimal,
}

impl Position {
    pub fn flat(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            side: None,
            quantity: Decimal::ZERO,
            avg_entry: Decimal::ZERO,
            realised_pnl: Decimal::ZERO,
            unrealised_pnl: Decimal::ZERO,
            last_mark: Decimal::ZERO,
        }
    }

    pub fn is_flat(&self) -> bool {
        self.side.is_none() || self.quantity == Decimal::ZERO
    }

    /// Apply a fill (signed by `side`). Returns the realised pnl
    /// produced by this fill (zero when the fill only adds to the
    /// existing direction).
    pub fn apply_fill(&mut self, side: Side, qty: Decimal, price: Decimal, fee: Decimal) -> Decimal {
        // Fees are always a cost: subtract from realised before
        // returning so the caller's running pnl includes them.
        let mut realised = -fee;
        let signed_qty = signed(side, qty);
        let current_signed = self
            .side
            .map(|s| signed(s, self.quantity))
            .unwrap_or(Decimal::ZERO);
        let new_signed = current_signed + signed_qty;

        if same_or_zero(current_signed, new_signed) {
            // Pure add (or first open).
            let abs_old = current_signed.abs();
            let abs_new = new_signed.abs();
            if abs_new == Decimal::ZERO {
                self.side = None;
                self.quantity = Decimal::ZERO;
                self.avg_entry = Decimal::ZERO;
            } else if abs_old == Decimal::ZERO {
                self.side = Some(side);
                self.quantity = qty;
                self.avg_entry = price;
            } else {
                // Adding to existing direction → weighted avg.
                let new_qty = abs_new;
                self.avg_entry =
                    ((self.avg_entry * abs_old) + (price * qty)) / new_qty;
                self.quantity = new_qty;
                // direction unchanged
            }
        } else {
            // Reducing or flipping.
            let close_qty = qty.min(self.quantity);
            let pnl_per_unit = match self.side {
                Some(Side::Long) => price - self.avg_entry,
                Some(Side::Short) => self.avg_entry - price,
                None => Decimal::ZERO,
            };
            realised += pnl_per_unit * close_qty;

            let leftover = qty - close_qty;
            self.quantity -= close_qty;
            if self.quantity == Decimal::ZERO {
                self.side = None;
                self.avg_entry = Decimal::ZERO;
            }
            if leftover > Decimal::ZERO {
                // Flip: open the residual on the new side at this price.
                self.side = Some(side);
                self.quantity = leftover;
                self.avg_entry = price;
            }
        }

        self.realised_pnl += realised;
        // Refresh unrealised against last known mark.
        self.mark(self.last_mark);
        realised
    }

    /// Mark to market against `mark` and refresh `unrealised_pnl`.
    pub fn mark(&mut self, mark: Decimal) {
        self.last_mark = mark;
        if self.is_flat() || mark == Decimal::ZERO {
            self.unrealised_pnl = Decimal::ZERO;
            return;
        }
        let pnl = match self.side.unwrap() {
            Side::Long => (mark - self.avg_entry) * self.quantity,
            Side::Short => (self.avg_entry - mark) * self.quantity,
        };
        self.unrealised_pnl = pnl;
    }
}

fn signed(side: Side, qty: Decimal) -> Decimal {
    match side {
        Side::Long => qty,
        Side::Short => -qty,
    }
}

/// True when both signs agree (or one of them is zero).
fn same_or_zero(a: Decimal, b: Decimal) -> bool {
    if a == Decimal::ZERO || b == Decimal::ZERO {
        return true;
    }
    (a > Decimal::ZERO) == (b > Decimal::ZERO)
}
