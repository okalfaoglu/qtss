use rust_decimal::Decimal;

/// `qty * mark` bu üst sınırı aşarsa miktar küçültülür.
#[must_use]
pub fn clamp_qty_by_max_notional_usdt(qty: Decimal, mark: Decimal, max_notional_usdt: Decimal) -> Decimal {
    if mark <= Decimal::ZERO || qty <= Decimal::ZERO {
        return qty;
    }
    if max_notional_usdt <= Decimal::ZERO {
        return qty;
    }
    let notional = qty * mark;
    if notional <= max_notional_usdt {
        qty
    } else {
        max_notional_usdt / mark
    }
}

/// Kelly çarpanı (1.0 = değişmez).
#[must_use]
pub fn kelly_qty_scale(kelly_apply: bool, win_rate: f64, avg_win_loss_ratio: f64, max_fraction: f64) -> f64 {
    if !kelly_apply {
        return 1.0;
    }
    let k = kelly_position_fraction(win_rate, avg_win_loss_ratio, max_fraction);
    if k <= 0.0 {
        0.0
    } else {
        k.min(1.0)
    }
}

/// Kelly-style position fraction. `max_fraction` caps exposure (e.g. quarter Kelly).
#[must_use]
pub fn kelly_position_fraction(win_rate: f64, avg_win_loss_ratio: f64, max_fraction: f64) -> f64 {
    if !win_rate.is_finite()
        || !avg_win_loss_ratio.is_finite()
        || avg_win_loss_ratio <= 1e-12
        || !max_fraction.is_finite()
    {
        return 0.0;
    }
    let kelly = win_rate - (1.0 - win_rate) / avg_win_loss_ratio;
    kelly.clamp(0.0, max_fraction)
}

#[must_use]
pub fn apply_kelly_scale_to_qty(qty: Decimal, kelly_scale: f64) -> Decimal {
    if kelly_scale <= 0.0 || !kelly_scale.is_finite() {
        return Decimal::ZERO;
    }
    let s = Decimal::from_f64_retain(kelly_scale).unwrap_or(Decimal::ONE);
    if s <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    qty * s
}

pub struct DrawdownGuard {
    pub max_daily_loss_pct: f64,
    pub current_daily_pnl_pct: f64,
}

impl DrawdownGuard {
    #[must_use]
    pub fn allows_new_position(&self) -> bool {
        self.current_daily_pnl_pct > -self.max_daily_loss_pct
    }

    #[must_use]
    pub fn new(max_daily_loss_pct: f64, current_daily_pnl_pct: f64) -> Self {
        Self {
            max_daily_loss_pct,
            current_daily_pnl_pct,
        }
    }
}

#[cfg(test)]
mod kelly_tests {
    use super::*;

    #[test]
    fn kelly_fraction_clamped_to_max() {
        let k = kelly_position_fraction(0.6, 2.0, 0.25);
        assert!(k > 0.0);
        assert!(k <= 0.25 + 1e-9);
    }
}
