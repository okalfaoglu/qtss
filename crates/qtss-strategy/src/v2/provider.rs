//! `StrategyProvider` trait + minimal evaluation context.

use crate::v2::error::StrategyResult;
use qtss_domain::v2::detection::ValidatedDetection;
use qtss_domain::v2::intent::{RunMode, TradeIntent};

/// Inputs every strategy gets in addition to the validated detection.
/// Kept tiny on purpose: anything venue-specific belongs in execution,
/// anything portfolio-wide belongs in risk.
#[derive(Debug, Clone)]
pub struct StrategyContext {
    pub run_mode: RunMode,
}

pub trait StrategyProvider: Send + Sync {
    /// Stable identifier — also used as `TradeIntent.strategy_id` and
    /// the registry key in the worker.
    fn id(&self) -> &str;

    /// Translate a validated detection into zero or more trade intents.
    /// Returning an empty `Vec` is a *pass*, not an error — the strategy
    /// looked but did not want to act. Errors are reserved for actually
    /// broken inputs / config.
    fn evaluate(
        &self,
        signal: &ValidatedDetection,
        ctx: &StrategyContext,
    ) -> StrategyResult<Vec<TradeIntent>>;
}
