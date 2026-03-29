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
