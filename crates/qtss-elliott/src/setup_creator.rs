//! Setup creator — convert validated patterns into tradeable setups.
//!
//! Takes Detection + validation + targets + position sizing rules,
//! outputs executable trade setup with position size, commission adjustment.

use crate::corrective::CorrectiveWave;
use crate::motive::MotiveWave;
use crate::validator::ValidationResult;

/// A tradeable setup ready for execution.
#[derive(Debug, Clone)]
pub struct TradeSetup {
    pub pattern_type: &'static str, // "motive" or "corrective"
    pub entry_price: f64,
    pub target_price: f64,
    pub stop_price: f64,
    pub position_size: f64, // in contracts/shares
    pub risk_amount: f64,   // $ or base asset
    pub reward_amount: f64,
    pub commission_cost: f64,
    /// After-commission profit target (reward - commission)
    pub net_target: f64,
    /// Setup is profitable after commission?
    pub is_viable: bool,
}

/// Position sizing rules (from config).
#[derive(Debug, Clone)]
pub struct PositionSizingRules {
    /// Account balance or capital available.
    pub account_size: f64,
    /// Max % of account to risk per trade.
    pub risk_percent: f64, // e.g., 2.0 = 2%
    /// Commission per trade (e.g., 0.001 = 0.1%).
    pub commission_rate: f64,
    /// Min profit needed to break even (absolute, not %).
    pub min_profit_threshold: f64,
}

/// Create setup from motive wave pattern.
pub fn create_motive_setup(
    motive: &MotiveWave,
    validation: &ValidationResult,
    target: f64,
    rules: &PositionSizingRules,
) -> TradeSetup {
    let stop = motive.points[0].price;
    let entry = motive.points[4].price;
    let risk_per_contract = (entry - stop).abs();

    // Position size: max risk_amount / risk_per_contract
    let risk_amount = (rules.account_size * rules.risk_percent) / 100.0;
    let position_size = (risk_amount / risk_per_contract).floor();

    let commission = entry * position_size * rules.commission_rate;
    let reward = (target - entry).abs() * position_size;
    let net_target = reward - commission;
    let is_viable = net_target >= rules.min_profit_threshold;

    TradeSetup {
        pattern_type: "motive",
        entry_price: entry,
        target_price: target,
        stop_price: stop,
        position_size,
        risk_amount,
        reward_amount: reward,
        commission_cost: commission,
        net_target,
        is_viable,
    }
}

/// Create setup from corrective wave pattern.
pub fn create_corrective_setup(
    corr: &CorrectiveWave,
    validation: &ValidationResult,
    target: f64,
    rules: &PositionSizingRules,
) -> TradeSetup {
    let stop = corr.points[1].price; // B peak
    let entry = corr.points[2].price; // C end
    let risk_per_contract = (entry - stop).abs();

    let risk_amount = (rules.account_size * rules.risk_percent) / 100.0;
    let position_size = (risk_amount / risk_per_contract).floor();

    let commission = entry * position_size * rules.commission_rate;
    let reward = (target - entry).abs() * position_size;
    let net_target = reward - commission;
    let is_viable = net_target >= rules.min_profit_threshold;

    TradeSetup {
        pattern_type: "corrective",
        entry_price: entry,
        target_price: target,
        stop_price: stop,
        position_size,
        risk_amount,
        reward_amount: reward,
        commission_cost: commission,
        net_target,
        is_viable,
    }
}
