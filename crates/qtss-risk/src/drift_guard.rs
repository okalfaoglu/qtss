//! Faz 9.8.10 — Drift, slippage, and funding guards.
//!
//! Three pure evaluators, each driving one dispatchable concern:
//!
//! - [`assess_price_drift`] — between setup creation and execution,
//!   has the mark drifted beyond a configured band? Used pre-entry.
//! - [`assess_slippage`]   — after a fill, is realised vs. expected
//!   slippage outside the tolerated envelope? Used post-trade.
//! - [`assess_funding`]    — for perpetuals, is the next funding rate
//!   so adverse that holding is more expensive than exiting?
//!
//! All three follow the same shape: `Config` struct + a `assess_*`
//! function returning `Option<GuardOutcome>` with a severity tag so
//! the worker can dispatch actions uniformly (CLAUDE.md #1).

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardSeverity {
    Info,
    Warn,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardOutcome {
    pub tag: &'static str,
    pub severity: GuardSeverity,
    pub reason: String,
    /// Normalised magnitude — fraction when the reason is percent-based.
    pub magnitude: f64,
}

// ---------------------------------------------------------------------------
// 1) Price drift — setup price vs. latest mark (pre-entry).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DriftConfig {
    /// Abandon the entry above this drift fraction.
    pub max_drift_pct: f64,
    /// Warn above this fraction (still allowed).
    pub warn_drift_pct: f64,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self { max_drift_pct: 0.015, warn_drift_pct: 0.005 }
    }
}

pub fn assess_price_drift(
    setup_price: Decimal,
    mark: Decimal,
    cfg: &DriftConfig,
) -> Option<GuardOutcome> {
    let sp = setup_price.to_f64()?;
    let m = mark.to_f64()?;
    if sp <= 0.0 { return None; }
    let drift = ((m - sp) / sp).abs();
    let severity = classify(drift, cfg.warn_drift_pct, cfg.max_drift_pct)?;
    Some(GuardOutcome {
        tag: "price_drift",
        severity,
        reason: format!("mark drifted {:.4} vs setup price {sp:.4}", drift),
        magnitude: drift,
    })
}

// ---------------------------------------------------------------------------
// 2) Slippage — realised vs. expected fill (post-trade).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SlippageConfig {
    /// Adverse slippage fraction that throws a critical alert.
    pub max_slippage_pct: f64,
    /// Warn above this.
    pub warn_slippage_pct: f64,
}

impl Default for SlippageConfig {
    fn default() -> Self {
        Self { max_slippage_pct: 0.005, warn_slippage_pct: 0.002 }
    }
}

pub fn assess_slippage(
    expected_price: Decimal,
    filled_price: Decimal,
    is_long: bool,
    cfg: &SlippageConfig,
) -> Option<GuardOutcome> {
    let e = expected_price.to_f64()?;
    let f = filled_price.to_f64()?;
    if e <= 0.0 { return None; }
    // Adverse direction only — a favourable slip is free alpha, not a guard trigger.
    let adverse = if is_long { (f - e) / e } else { (e - f) / e };
    if adverse <= 0.0 { return None; }
    let severity = classify(adverse, cfg.warn_slippage_pct, cfg.max_slippage_pct)?;
    Some(GuardOutcome {
        tag: "slippage",
        severity,
        reason: format!("adverse slippage {adverse:.4} (expected {e:.4} → filled {f:.4})"),
        magnitude: adverse,
    })
}

// ---------------------------------------------------------------------------
// 3) Funding — next-funding rate vs. carry tolerance (perps).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FundingConfig {
    /// Adverse funding-rate fraction per interval that triggers a
    /// critical alert (e.g. 0.001 = 0.1% per 8h).
    pub max_adverse_rate: f64,
    pub warn_adverse_rate: f64,
}

impl Default for FundingConfig {
    fn default() -> Self {
        Self { max_adverse_rate: 0.001, warn_adverse_rate: 0.0003 }
    }
}

pub fn assess_funding(
    next_rate: Option<Decimal>,
    is_long: bool,
    cfg: &FundingConfig,
) -> Option<GuardOutcome> {
    let rate = next_rate?.to_f64()?;
    // For longs, positive funding = you pay. For shorts, negative = you pay.
    let adverse = if is_long { rate } else { -rate };
    if adverse <= 0.0 { return None; }
    let severity = classify(adverse, cfg.warn_adverse_rate, cfg.max_adverse_rate)?;
    Some(GuardOutcome {
        tag: "funding",
        severity,
        reason: format!("adverse funding rate {adverse:.6} (next interval)"),
        magnitude: adverse,
    })
}

// ---------------------------------------------------------------------------

fn classify(value: f64, warn: f64, crit: f64) -> Option<GuardSeverity> {
    if value >= crit { Some(GuardSeverity::Critical) }
    else if value >= warn { Some(GuardSeverity::Warn) }
    else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn drift_warn_above_threshold() {
        let out = assess_price_drift(dec!(100), dec!(100.8), &DriftConfig::default()).unwrap();
        assert_eq!(out.severity, GuardSeverity::Warn);
    }

    #[test]
    fn drift_critical_beyond_max() {
        let out = assess_price_drift(dec!(100), dec!(102), &DriftConfig::default()).unwrap();
        assert_eq!(out.severity, GuardSeverity::Critical);
    }

    #[test]
    fn drift_noop_below_warn() {
        assert!(assess_price_drift(dec!(100), dec!(100.1), &DriftConfig::default()).is_none());
    }

    #[test]
    fn slippage_only_fires_on_adverse() {
        // Long, filled below expected → favourable → no alert.
        assert!(assess_slippage(dec!(100), dec!(99.5), true, &SlippageConfig::default()).is_none());
        // Long, filled above expected → adverse 1% → Critical.
        let out = assess_slippage(dec!(100), dec!(101), true, &SlippageConfig::default()).unwrap();
        assert_eq!(out.severity, GuardSeverity::Critical);
    }

    #[test]
    fn slippage_short_inverts_sign() {
        // Short filled above expected = favourable (sold higher), skipped.
        assert!(assess_slippage(dec!(100), dec!(101), false, &SlippageConfig::default()).is_none());
        // Short filled below expected = adverse (sold cheaper).
        let out = assess_slippage(dec!(100), dec!(99.7), false, &SlippageConfig::default()).unwrap();
        assert_eq!(out.severity, GuardSeverity::Warn);
    }

    #[test]
    fn funding_long_pays_positive_rate() {
        let out = assess_funding(Some(dec!(0.0012)), true, &FundingConfig::default()).unwrap();
        assert_eq!(out.severity, GuardSeverity::Critical);
    }

    #[test]
    fn funding_short_pays_negative_rate() {
        let out = assess_funding(Some(dec!(-0.0012)), false, &FundingConfig::default()).unwrap();
        assert_eq!(out.severity, GuardSeverity::Critical);
    }

    #[test]
    fn funding_skips_when_favourable() {
        assert!(assess_funding(Some(dec!(0.0012)), false, &FundingConfig::default()).is_none());
        assert!(assess_funding(None, true, &FundingConfig::default()).is_none());
    }
}
