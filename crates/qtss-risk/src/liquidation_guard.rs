//! Faz 9.8.3 — Liquidation guard full body.
//!
//! Evaluates the distance between `last_mark` and `liquidation_price`
//! for a live futures/margin position and emits a `LiquidationAction`.
//!
//! The severity ladder (warn / critical / breach) maps to actions via
//! a small dispatch table keyed on *margin headroom*:
//!   - breach          → `PanicClose`
//!   - critical + room → `AddMargin`
//!   - critical, tight → `ScaleOut`
//!   - warn            → `Alert`
//!   - safe            → `None`
//!
//! `assess()` is pure (no I/O). Callers persist the returned
//! [`LiquidationAssessment`] through [`LiquidationEventDto::from_assessment`]
//! → `qtss-storage::liquidation_guard_events::insert` when severity is
//! non-Safe. Spot short-circuits (can't be liquidated).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::live_position_store::LivePositionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiquidationSeverity {
    /// No action — distance is comfortable.
    Safe,
    /// First alert threshold (default 8%).
    Warn,
    /// Auto-action threshold (default 4%) — add margin / scale out.
    Critical,
    /// Panic-close threshold (default 1.5%) — market out now.
    Breach,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiquidationAction {
    None,
    Alert,
    AddMargin,
    ScaleOut,
    PanicClose,
}

#[derive(Debug, Clone)]
pub struct LiquidationGuardConfig {
    pub warn_distance_pct: Decimal,
    pub critical_distance_pct: Decimal,
    pub panic_close_distance_pct: Decimal,
    /// Free margin (quote currency) at or above this threshold lets
    /// the Critical severity pick `AddMargin` rather than `ScaleOut`.
    /// Defaults to 0 — without explicit headroom info we scale out.
    pub min_add_margin_headroom: Decimal,
}

impl Default for LiquidationGuardConfig {
    fn default() -> Self {
        Self {
            warn_distance_pct: Decimal::new(8, 2),     // 0.08
            critical_distance_pct: Decimal::new(4, 2), // 0.04
            panic_close_distance_pct: Decimal::new(15, 3), // 0.015
            min_add_margin_headroom: Decimal::ZERO,
        }
    }
}

/// Extra context needed by the action picker that isn't on
/// `LivePositionState`. Callers populate this from the exchange
/// account snapshot (free margin) at the moment of assessment.
#[derive(Debug, Clone, Copy, Default)]
pub struct MarginContext {
    /// Free margin available for top-up (quote currency). None when
    /// unknown — treated as "no room" (scale-out path).
    pub free_margin: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct LiquidationAssessment {
    pub severity: LiquidationSeverity,
    pub action: LiquidationAction,
    pub mark: Decimal,
    pub liquidation: Decimal,
    pub distance_pct: Decimal,
}

/// Pure evaluator — no I/O. Caller persists outcome to
/// `liquidation_guard_events` when severity != Safe.
///
/// Back-compat wrapper: assumes no margin headroom (`ScaleOut` chosen
/// on Critical). Prefer [`assess_with_margin`] when the account
/// snapshot is available.
pub fn assess(
    state: &LivePositionState,
    cfg: &LiquidationGuardConfig,
) -> Option<LiquidationAssessment> {
    assess_with_margin(state, cfg, MarginContext::default())
}

/// Full evaluator — picks `AddMargin` vs `ScaleOut` on Critical based
/// on whether `margin.free_margin` clears `cfg.min_add_margin_headroom`.
pub fn assess_with_margin(
    state: &LivePositionState,
    cfg: &LiquidationGuardConfig,
    margin: MarginContext,
) -> Option<LiquidationAssessment> {
    // Spot can never liquidate — short-circuit so misconfigured
    // `liquidation_price` values on spot positions don't trigger
    // panic-closes.
    if !state.segment.can_liquidate() {
        return None;
    }
    let mark = state.last_mark?;
    let liq = state.liquidation_price?;
    if mark <= Decimal::ZERO || liq <= Decimal::ZERO {
        return None;
    }
    // Distance is always reported as a positive fraction — the side
    // determines which direction "closer to liquidation" means, but
    // the magnitude alone drives the severity ladder.
    let diff = if liq > mark { liq - mark } else { mark - liq };
    let distance_pct = diff / mark;

    let severity = if distance_pct <= cfg.panic_close_distance_pct {
        LiquidationSeverity::Breach
    } else if distance_pct <= cfg.critical_distance_pct {
        LiquidationSeverity::Critical
    } else if distance_pct <= cfg.warn_distance_pct {
        LiquidationSeverity::Warn
    } else {
        LiquidationSeverity::Safe
    };

    let action = pick_action(severity, &margin, cfg);

    Some(LiquidationAssessment {
        severity,
        action,
        mark,
        liquidation: liq,
        distance_pct,
    })
}

/// Severity → action dispatch (CLAUDE.md #1: no inline if/else).
/// Critical branches on margin availability. Breach always panic-closes.
fn pick_action(
    severity: LiquidationSeverity,
    margin: &MarginContext,
    cfg: &LiquidationGuardConfig,
) -> LiquidationAction {
    match severity {
        LiquidationSeverity::Safe => LiquidationAction::None,
        LiquidationSeverity::Warn => LiquidationAction::Alert,
        LiquidationSeverity::Breach => LiquidationAction::PanicClose,
        LiquidationSeverity::Critical => critical_action(margin, cfg),
    }
}

fn critical_action(margin: &MarginContext, cfg: &LiquidationGuardConfig) -> LiquidationAction {
    match margin.free_margin {
        Some(free) if free >= cfg.min_add_margin_headroom && free > Decimal::ZERO => {
            LiquidationAction::AddMargin
        }
        _ => LiquidationAction::ScaleOut,
    }
}

// ---------------------------------------------------------------------------
// Persistence DTO — pure; the storage layer maps this to an INSERT.
// The DTO shape mirrors the `liquidation_guard_events` columns.
// ---------------------------------------------------------------------------

/// Severity tag persisted to DB. Our in-memory enum has a `Safe`
/// variant that is never persisted; `Critical` maps to the SQL
/// `'high'` label (migration constraint).
pub fn severity_db_tag(sev: LiquidationSeverity) -> Option<&'static str> {
    match sev {
        LiquidationSeverity::Safe => None,
        LiquidationSeverity::Warn => Some("warn"),
        LiquidationSeverity::Critical => Some("high"),
        LiquidationSeverity::Breach => Some("breach"),
    }
}

pub fn action_db_tag(action: LiquidationAction) -> &'static str {
    match action {
        LiquidationAction::None => "none",
        LiquidationAction::Alert => "alert",
        LiquidationAction::AddMargin => "add_margin",
        LiquidationAction::ScaleOut => "scale_out",
        LiquidationAction::PanicClose => "panic_close",
    }
}

#[derive(Debug, Clone)]
pub struct LiquidationEventDto {
    pub position_id: Uuid,
    pub severity: &'static str,
    pub action: &'static str,
    pub mark_price: Decimal,
    pub liquidation_price: Decimal,
    pub distance_pct: Decimal,
    pub margin_ratio: Option<Decimal>,
    pub free_margin: Option<Decimal>,
}

impl LiquidationEventDto {
    /// Build a persistable row from an assessment. Returns `None` when
    /// severity is `Safe` (nothing to persist).
    pub fn from_assessment(
        position_id: Uuid,
        state: &LivePositionState,
        assessment: &LiquidationAssessment,
        margin: &MarginContext,
    ) -> Option<Self> {
        let severity = severity_db_tag(assessment.severity)?;
        Some(Self {
            position_id,
            severity,
            action: action_db_tag(assessment.action),
            mark_price: assessment.mark,
            liquidation_price: assessment.liquidation,
            distance_pct: assessment.distance_pct,
            margin_ratio: state.maint_margin_ratio,
            free_margin: margin.free_margin,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_position_store::{ExecutionMode, MarketSegment, PositionSide};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    fn futures_state(mark: Decimal, liq: Decimal) -> LivePositionState {
        LivePositionState {
            id: Uuid::new_v4(),
            setup_id: None,
            mode: ExecutionMode::Dry,
            exchange: "binance".into(),
            segment: MarketSegment::Futures,
            symbol: "BTCUSDT".into(),
            side: PositionSide::Buy,
            leverage: 10,
            entry_avg: dec!(100),
            qty_filled: dec!(1),
            qty_remaining: dec!(1),
            current_sl: Some(dec!(95)),
            tp_ladder: Vec::new(),
            liquidation_price: Some(liq),
            maint_margin_ratio: Some(dec!(0.005)),
            funding_rate_next: None,
            last_mark: Some(mark),
            last_tick_at: Some(Utc::now()),
            opened_at: Utc::now(),
        }
    }

    #[test]
    fn breach_forces_panic_close_regardless_of_margin() {
        let s = futures_state(dec!(100), dec!(99));
        let margin = MarginContext { free_margin: Some(dec!(10_000)) };
        let a = assess_with_margin(&s, &LiquidationGuardConfig::default(), margin).unwrap();
        assert_eq!(a.severity, LiquidationSeverity::Breach);
        assert_eq!(a.action, LiquidationAction::PanicClose);
    }

    #[test]
    fn critical_with_margin_picks_add_margin() {
        // 3% distance → Critical (below 4% threshold, above 1.5% breach).
        let s = futures_state(dec!(100), dec!(97));
        let margin = MarginContext { free_margin: Some(dec!(500)) };
        let a = assess_with_margin(&s, &LiquidationGuardConfig::default(), margin).unwrap();
        assert_eq!(a.severity, LiquidationSeverity::Critical);
        assert_eq!(a.action, LiquidationAction::AddMargin);
    }

    #[test]
    fn critical_without_margin_picks_scale_out() {
        let s = futures_state(dec!(100), dec!(97));
        let margin = MarginContext { free_margin: None };
        let a = assess_with_margin(&s, &LiquidationGuardConfig::default(), margin).unwrap();
        assert_eq!(a.action, LiquidationAction::ScaleOut);
    }

    #[test]
    fn critical_below_headroom_picks_scale_out() {
        let s = futures_state(dec!(100), dec!(97));
        let mut cfg = LiquidationGuardConfig::default();
        cfg.min_add_margin_headroom = dec!(1000);
        let margin = MarginContext { free_margin: Some(dec!(50)) };
        let a = assess_with_margin(&s, &cfg, margin).unwrap();
        assert_eq!(a.action, LiquidationAction::ScaleOut);
    }

    #[test]
    fn warn_alerts_only() {
        // 6% distance → Warn (<8%, >4%).
        let s = futures_state(dec!(100), dec!(94));
        let a = assess(&s, &LiquidationGuardConfig::default()).unwrap();
        assert_eq!(a.severity, LiquidationSeverity::Warn);
        assert_eq!(a.action, LiquidationAction::Alert);
    }

    #[test]
    fn safe_distance_returns_none_action() {
        // 15% distance → Safe.
        let s = futures_state(dec!(100), dec!(85));
        let a = assess(&s, &LiquidationGuardConfig::default()).unwrap();
        assert_eq!(a.severity, LiquidationSeverity::Safe);
        assert_eq!(a.action, LiquidationAction::None);
    }

    #[test]
    fn dto_skips_safe_severity() {
        let s = futures_state(dec!(100), dec!(85));
        let a = assess(&s, &LiquidationGuardConfig::default()).unwrap();
        let dto = LiquidationEventDto::from_assessment(s.id, &s, &a, &MarginContext::default());
        assert!(dto.is_none());
    }

    #[test]
    fn dto_maps_critical_to_high_severity() {
        let s = futures_state(dec!(100), dec!(97));
        let a = assess(&s, &LiquidationGuardConfig::default()).unwrap();
        let dto = LiquidationEventDto::from_assessment(
            s.id, &s, &a, &MarginContext::default(),
        ).unwrap();
        assert_eq!(dto.severity, "high");
        assert_eq!(dto.action, "scale_out");
    }

    #[test]
    fn spot_never_assesses() {
        let mut s = futures_state(dec!(100), dec!(97));
        s.segment = MarketSegment::Spot;
        assert!(assess(&s, &LiquidationGuardConfig::default()).is_none());
    }
}
