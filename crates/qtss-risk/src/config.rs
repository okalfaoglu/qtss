//! Risk configuration. Mirrors the keys seeded by migration 0016 in
//! `qtss_config` (CLAUDE.md rule #2 — no hardcoded values).

use crate::error::{RiskError, RiskResult};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Debug, Clone)]
pub struct RiskConfig {
    /// Hard kill-switch on equity drawdown — once breached the engine
    /// rejects every intent regardless of side or strategy.
    pub killswitch_drawdown: Decimal,
    /// Maximum acceptable session-level drawdown before further entries
    /// are halted (softer than the kill switch).
    pub max_drawdown: Decimal,
    /// Maximum acceptable daily PnL loss as a fraction of starting equity.
    pub max_daily_loss: Decimal,
    /// Hard cap on the number of simultaneously open positions.
    pub max_open_positions: u32,
    /// Account-wide leverage cap. `1.0` = spot only.
    pub max_leverage: Decimal,
    /// Cap on per-trade risk (stop distance × quantity / equity), as a
    /// fraction. The sizer trims `quantity` to this number when needed.
    pub max_risk_per_trade: Decimal,
}

impl RiskConfig {
    pub fn defaults() -> Self {
        Self {
            killswitch_drawdown: dec!(0.08),
            max_drawdown: dec!(0.05),
            max_daily_loss: dec!(0.02),
            max_open_positions: 8,
            max_leverage: dec!(1.0),
            max_risk_per_trade: dec!(0.01),
        }
    }

    pub fn validate(&self) -> RiskResult<()> {
        if self.killswitch_drawdown <= Decimal::ZERO {
            return Err(RiskError::InvalidConfig(
                "killswitch_drawdown must be > 0".into(),
            ));
        }
        if self.max_drawdown <= Decimal::ZERO || self.max_drawdown > self.killswitch_drawdown {
            return Err(RiskError::InvalidConfig(
                "max_drawdown must be > 0 and <= killswitch_drawdown".into(),
            ));
        }
        if self.max_daily_loss <= Decimal::ZERO {
            return Err(RiskError::InvalidConfig(
                "max_daily_loss must be > 0".into(),
            ));
        }
        if self.max_open_positions == 0 {
            return Err(RiskError::InvalidConfig(
                "max_open_positions must be > 0".into(),
            ));
        }
        if self.max_leverage <= Decimal::ZERO {
            return Err(RiskError::InvalidConfig(
                "max_leverage must be > 0".into(),
            ));
        }
        if self.max_risk_per_trade <= Decimal::ZERO {
            return Err(RiskError::InvalidConfig(
                "max_risk_per_trade must be > 0".into(),
            ));
        }
        Ok(())
    }
}
