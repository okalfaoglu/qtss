//! Faz 9.8.2 — Risk allocator + commission gate + drawdown guard.
//!
//! Sits after the selector (9.8.1) and before the execution manager
//! (9.8.4). Takes an approved [`SetupCandidate`] plus the live
//! [`AccountState`] and decides two things:
//!   1. Is opening a new position *allowed right now* (session
//!      drawdown, equity floor, per-symbol cap, commission gate)?
//!   2. If so, *how much notional* to commit (sizing).
//!
//! Gates dispatch through the [`AllocatorGate`] trait (CLAUDE.md #1).
//! The allocator itself is pure — callers hand in fees + equity; no
//! DB access here.

use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::selector::{Direction, SetupCandidate};
use crate::state::AccountState;

/// Per-venue commission rates. Futures and spot are often different;
/// caller supplies the right bundle for the candidate's segment.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommissionRates {
    /// Fraction charged on maker-side fills (e.g. 0.0002 = 2 bps).
    pub maker_bps: f64,
    /// Fraction charged on taker-side fills.
    pub taker_bps: f64,
}

impl CommissionRates {
    /// Total round-trip fee as fraction of notional, assuming maker
    /// entry + taker exit (the common real-world case for a bracket
    /// order with market-out on SL/TP).
    pub fn round_trip(&self) -> f64 {
        self.maker_bps + self.taker_bps
    }
}

#[derive(Debug, Clone)]
pub struct AllocatorConfig {
    /// Session-drawdown cap — block new entries when current drawdown
    /// exceeds this fraction of peak equity.
    pub max_session_drawdown: f64,
    /// Equity floor — never open new positions below this absolute equity.
    pub min_equity: Decimal,
    /// Cap on gross notional as fraction of equity across all open
    /// positions. Soft cap — allocator trims size if exceeded.
    pub max_gross_exposure: f64,
    /// Expected net profit / (commission + slippage buffer) must exceed
    /// this ratio for the trade to survive the commission gate.
    pub min_edge_ratio: f64,
    /// Slippage buffer added on top of commission in the edge calc.
    pub slippage_bps: f64,
}

impl Default for AllocatorConfig {
    fn default() -> Self {
        Self {
            max_session_drawdown: 0.10, // 10%
            min_equity: Decimal::new(100, 0),
            max_gross_exposure: 3.0,
            min_edge_ratio: 1.5,
            slippage_bps: 10.0 / 10_000.0, // 10 bps
        }
    }
}

/// Everything the allocator needs beyond the candidate itself.
#[derive(Debug, Clone)]
pub struct AllocationInput {
    pub candidate: SetupCandidate,
    pub account: AccountState,
    pub commission: CommissionRates,
    /// Aggregate gross notional already deployed (quote currency).
    pub open_notional: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AllocationOutcome {
    Approved {
        setup_id: Uuid,
        qty: Decimal,
        notional: Decimal,
        /// Expected net profit at TP, net of fees + slippage buffer.
        expected_net_profit: Decimal,
    },
    Rejected {
        setup_id: Uuid,
        gate: &'static str,
        reason: String,
    },
}

impl AllocationOutcome {
    pub fn is_approved(&self) -> bool {
        matches!(self, AllocationOutcome::Approved { .. })
    }
    pub fn setup_id(&self) -> Uuid {
        match self {
            AllocationOutcome::Approved { setup_id, .. }
            | AllocationOutcome::Rejected { setup_id, .. } => *setup_id,
        }
    }
}

/// Single pre-sizing gate. Returns `Some(reason)` to reject.
pub trait AllocatorGate: Send + Sync {
    fn tag(&self) -> &'static str;
    fn evaluate(&self, input: &AllocationInput, cfg: &AllocatorConfig) -> Option<String>;
}

// ---------------------------------------------------------------------------
// Gates (CLAUDE.md #1)
// ---------------------------------------------------------------------------

pub struct DrawdownGate;

impl AllocatorGate for DrawdownGate {
    fn tag(&self) -> &'static str {
        "drawdown"
    }
    fn evaluate(&self, input: &AllocationInput, cfg: &AllocatorConfig) -> Option<String> {
        let dd = input.account.drawdown().to_f64().unwrap_or(0.0);
        if dd >= cfg.max_session_drawdown {
            return Some(format!(
                "session drawdown {:.2}% >= cap {:.2}%",
                dd * 100.0,
                cfg.max_session_drawdown * 100.0
            ));
        }
        None
    }
}

pub struct EquityFloorGate;

impl AllocatorGate for EquityFloorGate {
    fn tag(&self) -> &'static str {
        "equity_floor"
    }
    fn evaluate(&self, input: &AllocationInput, cfg: &AllocatorConfig) -> Option<String> {
        if input.account.equity < cfg.min_equity {
            return Some(format!(
                "equity {} < floor {}",
                input.account.equity, cfg.min_equity
            ));
        }
        None
    }
}

pub struct CommissionGate;

impl AllocatorGate for CommissionGate {
    fn tag(&self) -> &'static str {
        "commission"
    }
    fn evaluate(&self, input: &AllocationInput, cfg: &AllocatorConfig) -> Option<String> {
        // Use percentage-based edge — size-independent, so the gate
        // decision is invariant to notional scaling.
        let entry = decimal_to_f64(input.candidate.entry_price);
        let target = decimal_to_f64(input.candidate.target_price);
        let gross_pct = match input.candidate.direction {
            Direction::Long if entry > 0.0 => (target - entry) / entry,
            Direction::Short if entry > 0.0 => (entry - target) / entry,
            _ => return Some("non-positive entry price".into()),
        };
        if gross_pct <= 0.0 {
            return Some(format!(
                "non-positive gross_pct {gross_pct:.4} — direction/target mismatch"
            ));
        }
        let fee_pct = input.commission.round_trip() + cfg.slippage_bps;
        if fee_pct <= 0.0 {
            return None; // fee-free venue — nothing to gate on
        }
        let ratio = gross_pct / fee_pct;
        if ratio < cfg.min_edge_ratio {
            return Some(format!(
                "edge ratio {ratio:.2} < {:.2} (gross {:.4}, fee+slip {:.4})",
                cfg.min_edge_ratio, gross_pct, fee_pct
            ));
        }
        None
    }
}

pub struct MaxExposureGate;

impl AllocatorGate for MaxExposureGate {
    fn tag(&self) -> &'static str {
        "max_exposure"
    }
    fn evaluate(&self, input: &AllocationInput, cfg: &AllocatorConfig) -> Option<String> {
        let eq = decimal_to_f64(input.account.equity);
        if eq <= 0.0 {
            return Some("non-positive equity".into());
        }
        let gross = decimal_to_f64(input.open_notional);
        let ratio = gross / eq;
        if ratio >= cfg.max_gross_exposure {
            return Some(format!(
                "gross exposure {ratio:.2}x >= cap {:.2}x",
                cfg.max_gross_exposure
            ));
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Allocator — walks gates, then sizes
// ---------------------------------------------------------------------------

pub struct Allocator {
    gates: Vec<Box<dyn AllocatorGate>>,
}

impl Allocator {
    pub fn new() -> Self {
        Self { gates: Vec::new() }
    }

    /// Production set — equity floor → drawdown → exposure → commission.
    /// Ordered cheapest-first so hot-path rejections short-circuit fast.
    pub fn with_defaults() -> Self {
        let mut a = Self::new();
        a.register(Box::new(EquityFloorGate));
        a.register(Box::new(DrawdownGate));
        a.register(Box::new(MaxExposureGate));
        a.register(Box::new(CommissionGate));
        a
    }

    pub fn register(&mut self, g: Box<dyn AllocatorGate>) {
        self.gates.push(g);
    }

    pub fn evaluate(
        &self,
        input: &AllocationInput,
        cfg: &AllocatorConfig,
    ) -> AllocationOutcome {
        for g in &self.gates {
            if let Some(reason) = g.evaluate(input, cfg) {
                return AllocationOutcome::Rejected {
                    setup_id: input.candidate.setup_id,
                    gate: g.tag(),
                    reason,
                };
            }
        }
        size_position(input, cfg)
    }
}

impl Default for Allocator {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Size a position given an approved candidate. Uses `risk_pct` on
/// the candidate as the fraction of equity to risk, then clips against
/// the gross exposure cap. Returns an `Approved` outcome including
/// expected net profit at TP.
fn size_position(input: &AllocationInput, cfg: &AllocatorConfig) -> AllocationOutcome {
    let entry = decimal_to_f64(input.candidate.entry_price);
    let stop = decimal_to_f64(input.candidate.stop_price);
    let target = decimal_to_f64(input.candidate.target_price);
    let eq = decimal_to_f64(input.account.equity);
    let risk_per_unit = match input.candidate.direction {
        Direction::Long => entry - stop,
        Direction::Short => stop - entry,
    };
    if risk_per_unit <= 0.0 || entry <= 0.0 {
        return AllocationOutcome::Rejected {
            setup_id: input.candidate.setup_id,
            gate: "sizing",
            reason: "non-positive risk_per_unit or entry".into(),
        };
    }
    let dollar_risk = eq * input.candidate.risk_pct;
    let raw_qty = dollar_risk / risk_per_unit;

    // Exposure clip.
    let gross_open = decimal_to_f64(input.open_notional);
    let max_total = eq * cfg.max_gross_exposure;
    let max_new_notional = (max_total - gross_open).max(0.0);
    let notional_uncapped = raw_qty * entry;
    let notional = notional_uncapped.min(max_new_notional);
    if notional <= 0.0 {
        return AllocationOutcome::Rejected {
            setup_id: input.candidate.setup_id,
            gate: "sizing",
            reason: "exposure cap leaves no room".into(),
        };
    }
    let qty = notional / entry;

    // Net profit at TP = gross - commission - slippage.
    let gross_pct = match input.candidate.direction {
        Direction::Long => (target - entry) / entry,
        Direction::Short => (entry - target) / entry,
    };
    let fee_pct = input.commission.round_trip() + cfg.slippage_bps;
    let net_pct = gross_pct - fee_pct;
    let expected_net = notional * net_pct;

    AllocationOutcome::Approved {
        setup_id: input.candidate.setup_id,
        qty: f64_to_decimal(qty),
        notional: f64_to_decimal(notional),
        expected_net_profit: f64_to_decimal(expected_net),
    }
}

fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

fn f64_to_decimal(f: f64) -> Decimal {
    use std::str::FromStr;
    if !f.is_finite() {
        return Decimal::ZERO;
    }
    Decimal::from_str(&format!("{f:.8}")).unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_position_store::MarketSegment;
    use rust_decimal_macros::dec;

    fn acct(equity: Decimal, peak: Decimal, dd_pnl: Decimal) -> AccountState {
        AccountState {
            equity,
            peak_equity: peak,
            day_pnl: dd_pnl,
            open_positions: 0,
            current_leverage: Decimal::ONE,
            kill_switch_manual: false,
        }
    }

    fn cand(entry: Decimal, stop: Decimal, target: Decimal) -> SetupCandidate {
        SetupCandidate {
            setup_id: Uuid::new_v4(),
            exchange: "binance".into(),
            segment: MarketSegment::Futures,
            symbol: "BTCUSDT".into(),
            timeframe: "1h".into(),
            profile: "d".into(),
            direction: Direction::Long,
            entry_price: entry,
            stop_price: stop,
            target_price: target,
            ai_score: 0.80,
            risk_pct: 0.01,
            tier: 8,
            open_positions_on_symbol: 0,
            under_liquidation_cooldown: false,
        }
    }

    fn input(
        equity: Decimal,
        peak: Decimal,
        open_notional: Decimal,
        commission: CommissionRates,
    ) -> AllocationInput {
        AllocationInput {
            candidate: cand(dec!(100), dec!(98), dec!(106)),
            account: acct(equity, peak, Decimal::ZERO),
            commission,
            open_notional,
        }
    }

    #[test]
    fn approves_clean_candidate_with_positive_net() {
        let i = input(dec!(10_000), dec!(10_000), dec!(0), CommissionRates {
            maker_bps: 0.0002,
            taker_bps: 0.0004,
        });
        let a = Allocator::with_defaults();
        let out = a.evaluate(&i, &AllocatorConfig::default());
        match out {
            AllocationOutcome::Approved { qty, notional, expected_net_profit, .. } => {
                assert!(qty > Decimal::ZERO);
                assert!(notional > Decimal::ZERO);
                assert!(expected_net_profit > Decimal::ZERO);
            }
            other => panic!("expected approval, got {other:?}"),
        }
    }

    #[test]
    fn rejects_on_drawdown() {
        let i = input(dec!(8_000), dec!(10_000), dec!(0), CommissionRates::default());
        let mut cfg = AllocatorConfig::default();
        cfg.max_session_drawdown = 0.10; // 10% — current is 20%
        let a = Allocator::with_defaults();
        let out = a.evaluate(&i, &cfg);
        match out {
            AllocationOutcome::Rejected { gate, .. } => assert_eq!(gate, "drawdown"),
            other => panic!("expected drawdown rejection, got {other:?}"),
        }
    }

    #[test]
    fn rejects_on_commission_when_edge_too_thin() {
        // Target 0.06% above entry; round-trip fee 0.5% → way below edge ratio.
        let mut i = input(dec!(10_000), dec!(10_000), dec!(0), CommissionRates {
            maker_bps: 0.003,
            taker_bps: 0.002,
        });
        i.candidate.target_price = dec!(100.06);
        let a = Allocator::with_defaults();
        let out = a.evaluate(&i, &AllocatorConfig::default());
        match out {
            AllocationOutcome::Rejected { gate, .. } => assert_eq!(gate, "commission"),
            other => panic!("expected commission rejection, got {other:?}"),
        }
    }

    #[test]
    fn rejects_on_max_exposure() {
        // Already 3.0x — hit the cap.
        let i = input(dec!(10_000), dec!(10_000), dec!(30_000), CommissionRates::default());
        let a = Allocator::with_defaults();
        let out = a.evaluate(&i, &AllocatorConfig::default());
        match out {
            AllocationOutcome::Rejected { gate, .. } => assert_eq!(gate, "max_exposure"),
            other => panic!("expected exposure rejection, got {other:?}"),
        }
    }

    #[test]
    fn rejects_below_equity_floor() {
        let i = input(dec!(50), dec!(100), dec!(0), CommissionRates::default());
        let mut cfg = AllocatorConfig::default();
        cfg.min_equity = dec!(100);
        let a = Allocator::with_defaults();
        let out = a.evaluate(&i, &cfg);
        match out {
            AllocationOutcome::Rejected { gate, .. } => assert_eq!(gate, "equity_floor"),
            other => panic!("expected equity_floor rejection, got {other:?}"),
        }
    }

    #[test]
    fn exposure_cap_trims_notional_when_close_to_limit() {
        // Already at 2.9x of 10k = 29k. Cap 3x → only 1k new allowed.
        let i = input(dec!(10_000), dec!(10_000), dec!(29_000), CommissionRates::default());
        let a = Allocator::with_defaults();
        let out = a.evaluate(&i, &AllocatorConfig::default());
        match out {
            AllocationOutcome::Approved { notional, .. } => {
                assert!(notional <= dec!(1000), "expected trim ≤1000, got {notional}");
            }
            other => panic!("expected approval with trim, got {other:?}"),
        }
    }

    #[test]
    fn short_direction_sizes_correctly() {
        let mut i = input(dec!(10_000), dec!(10_000), dec!(0), CommissionRates {
            maker_bps: 0.0002, taker_bps: 0.0004,
        });
        i.candidate.direction = Direction::Short;
        i.candidate.entry_price = dec!(100);
        i.candidate.stop_price = dec!(102);
        i.candidate.target_price = dec!(94);
        let a = Allocator::with_defaults();
        let out = a.evaluate(&i, &AllocatorConfig::default());
        assert!(out.is_approved(), "short should approve with clean params");
    }
}
