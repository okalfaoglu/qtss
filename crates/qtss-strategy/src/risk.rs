use std::str::FromStr;

use rust_decimal::Decimal;

/// `QTSS_MAX_POSITION_NOTIONAL_USDT` — `qty * mark` bu üst sınırı aşarsa miktar küçültülür.
#[must_use]
pub fn clamp_qty_by_max_notional_usdt(qty: Decimal, mark: Decimal) -> Decimal {
    if mark <= Decimal::ZERO || qty <= Decimal::ZERO {
        return qty;
    }
    let max_n = match std::env::var("QTSS_MAX_POSITION_NOTIONAL_USDT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
    {
        Some(m) if m > Decimal::ZERO => m,
        _ => return qty,
    };
    let notional = qty * mark;
    if notional <= max_n {
        qty
    } else {
        max_n / mark
    }
}

/// Kelly çarpanı (1.0 = değişmez). `QTSS_KELLY_APPLY=1` iken env oranlarıyla [`kelly_position_fraction`].
#[must_use]
pub fn kelly_qty_scale_from_env() -> f64 {
    if !std::env::var("QTSS_KELLY_APPLY")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
    {
        return 1.0;
    }
    let win = std::env::var("QTSS_KELLY_WIN_RATE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.55_f64);
    let wl = std::env::var("QTSS_KELLY_AVG_WIN_LOSS_RATIO")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.5_f64);
    let cap = std::env::var("QTSS_KELLY_MAX_FRACTION")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.25_f64);
    let k = kelly_position_fraction(win, wl, cap);
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

/// `QTSS_STRATEGY_ORDER_QTY` vb. taban miktarı Kelly çarpanıyla çarpar (varsayılan çarpan 1).
#[must_use]
pub fn apply_kelly_scale_to_qty(qty: Decimal) -> Decimal {
    let scale = kelly_qty_scale_from_env();
    if scale <= 0.0 || !scale.is_finite() {
        return Decimal::ZERO;
    }
    let s = Decimal::from_f64_retain(scale).unwrap_or(Decimal::ONE);
    if s <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    qty * s
}

/// Daily drawdown guard from `QTSS_MAX_DRAWDOWN_PCT` (default 5.0).
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
    pub fn from_env(current_daily_pnl_pct: f64) -> Self {
        let max = std::env::var("QTSS_MAX_DRAWDOWN_PCT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5.0_f64);
        Self {
            max_daily_loss_pct: max,
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
