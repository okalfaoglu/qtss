//! Pre-trade checks.
//!
//! Each check implements [`RiskCheck`] and returns either `Ok(())` to
//! approve or `Err(RiskRejection)` to refuse. The engine walks every
//! registered check in order; the first rejection short-circuits.
//! Adding a new check is one impl + one `engine.register(...)` call —
//! no central match arm to edit (CLAUDE.md rule #1).

use crate::config::RiskConfig;
use crate::state::AccountState;
use qtss_domain::v2::intent::{RiskRejection, TradeIntent};

pub trait RiskCheck: Send + Sync {
    fn name(&self) -> &'static str;
    fn evaluate(
        &self,
        intent: &TradeIntent,
        state: &AccountState,
        cfg: &RiskConfig,
    ) -> Result<(), RiskRejection>;
}

// ---------------------------------------------------------------------------
// Kill switch
// ---------------------------------------------------------------------------

pub struct KillSwitchCheck;

impl RiskCheck for KillSwitchCheck {
    fn name(&self) -> &'static str {
        "kill_switch"
    }

    fn evaluate(
        &self,
        _intent: &TradeIntent,
        state: &AccountState,
        cfg: &RiskConfig,
    ) -> Result<(), RiskRejection> {
        if state.kill_switch_manual {
            return Err(RiskRejection::KillSwitchActive("manual".into()));
        }
        if state.drawdown() >= cfg.killswitch_drawdown {
            return Err(RiskRejection::KillSwitchActive(format!(
                "drawdown {} >= killswitch {}",
                state.drawdown(),
                cfg.killswitch_drawdown
            )));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Drawdown / day loss
// ---------------------------------------------------------------------------

pub struct DrawdownCheck;

impl RiskCheck for DrawdownCheck {
    fn name(&self) -> &'static str {
        "drawdown"
    }

    fn evaluate(
        &self,
        _intent: &TradeIntent,
        state: &AccountState,
        cfg: &RiskConfig,
    ) -> Result<(), RiskRejection> {
        let dd = state.drawdown();
        if dd >= cfg.max_drawdown {
            return Err(RiskRejection::DrawdownExceeded {
                dd_pct: dd.to_string(),
                cap_pct: cfg.max_drawdown.to_string(),
            });
        }
        Ok(())
    }
}

pub struct DailyLossCheck;

impl RiskCheck for DailyLossCheck {
    fn name(&self) -> &'static str {
        "daily_loss"
    }

    fn evaluate(
        &self,
        _intent: &TradeIntent,
        state: &AccountState,
        cfg: &RiskConfig,
    ) -> Result<(), RiskRejection> {
        let dl = state.day_loss_pct();
        if dl >= cfg.max_daily_loss {
            return Err(RiskRejection::MaxDailyLossReached {
                dd_pct: dl.to_string(),
                cap_pct: cfg.max_daily_loss.to_string(),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Open positions
// ---------------------------------------------------------------------------

pub struct MaxOpenPositionsCheck;

impl RiskCheck for MaxOpenPositionsCheck {
    fn name(&self) -> &'static str {
        "max_open_positions"
    }

    fn evaluate(
        &self,
        _intent: &TradeIntent,
        state: &AccountState,
        cfg: &RiskConfig,
    ) -> Result<(), RiskRejection> {
        if state.open_positions >= cfg.max_open_positions {
            return Err(RiskRejection::MaxOpenPositionsReached {
                current: state.open_positions,
                cap: cfg.max_open_positions,
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Leverage
// ---------------------------------------------------------------------------

pub struct LeverageCheck;

impl RiskCheck for LeverageCheck {
    fn name(&self) -> &'static str {
        "leverage"
    }

    fn evaluate(
        &self,
        _intent: &TradeIntent,
        state: &AccountState,
        cfg: &RiskConfig,
    ) -> Result<(), RiskRejection> {
        if state.current_leverage > cfg.max_leverage {
            return Err(RiskRejection::MaxLeverageExceeded {
                requested: state.current_leverage.to_string(),
                cap: cfg.max_leverage.to_string(),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Stop sanity
// ---------------------------------------------------------------------------

pub struct StopDistanceCheck;

impl RiskCheck for StopDistanceCheck {
    fn name(&self) -> &'static str {
        "stop_distance"
    }

    fn evaluate(
        &self,
        intent: &TradeIntent,
        _state: &AccountState,
        _cfg: &RiskConfig,
    ) -> Result<(), RiskRejection> {
        // Need an entry to compute distance against. If none is set
        // (market-on-next-bar), defer to runtime — pass.
        let entry = match intent.entry_price {
            Some(e) => e,
            None => return Ok(()),
        };
        let distance = (entry - intent.stop_loss).abs();
        if distance == rust_decimal::Decimal::ZERO {
            return Err(RiskRejection::StopDistanceTooSmall);
        }
        Ok(())
    }
}
