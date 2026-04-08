//! `/v2/dashboard` wire types (plan §9A.4).
//!
//! Maps the engine's [`PortfolioEngine::snapshot`] +
//! [`AccountState`] into the cards the React dashboard renders.
//! Anything beyond what a card actually shows is intentionally left
//! out so the contract stays small.

use chrono::{DateTime, Utc};
use qtss_portfolio::PortfolioEngine;
use qtss_risk::AccountState;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Aggregate portfolio card — top-left of the dashboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioCard {
    pub equity: Decimal,
    pub peak_equity: Decimal,
    pub day_pnl: Decimal,
    pub open_positions: u32,
    pub current_leverage: Decimal,
}

/// Risk usage card — top-right of the dashboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskCard {
    pub kill_switch_manual: bool,
    pub drawdown_pct: Decimal,
    pub current_leverage: Decimal,
    pub day_pnl: Decimal,
}

/// One row in the open-positions table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenPositionView {
    pub symbol: String,
    pub side: String,
    pub quantity: Decimal,
    pub avg_entry: Decimal,
    pub last_mark: Decimal,
    pub unrealised_pnl: Decimal,
    pub realised_pnl: Decimal,
}

/// One sample for the equity-curve area chart. Backed by the
/// portfolio engine's mark-to-market timeline; persistence and
/// down-sampling happen elsewhere — this struct is the wire shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquityPoint {
    pub at: DateTime<Utc>,
    pub equity: Decimal,
}

/// Whole `/v2/dashboard` payload — one round trip, no chained calls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DashboardSnapshot {
    pub generated_at: DateTime<Utc>,
    pub portfolio: PortfolioCard,
    pub risk: RiskCard,
    pub open_positions: Vec<OpenPositionView>,
    pub equity_curve: Vec<EquityPoint>,
}

impl DashboardSnapshot {
    /// Build a snapshot purely from in-memory engine state. The
    /// equity curve is supplied by the caller (storage or an in-memory
    /// ring buffer) — the engine itself does not retain history, on
    /// purpose. Keeping that decision out of this function lets the
    /// dashboard route swap persistence backends without touching the
    /// DTO module.
    pub fn build(
        portfolio: &PortfolioEngine,
        account: &AccountState,
        equity_curve: Vec<EquityPoint>,
    ) -> Self {
        let open_positions = portfolio
            .positions()
            .filter(|p| !p.is_flat())
            .map(|p| OpenPositionView {
                symbol: p.symbol.clone(),
                side: side_label(p.side),
                quantity: p.quantity,
                avg_entry: p.avg_entry,
                last_mark: p.last_mark,
                unrealised_pnl: p.unrealised_pnl,
                realised_pnl: p.realised_pnl,
            })
            .collect();

        let drawdown_pct = compute_drawdown_pct(account);

        Self {
            generated_at: Utc::now(),
            portfolio: PortfolioCard {
                equity: account.equity,
                peak_equity: account.peak_equity,
                day_pnl: account.day_pnl,
                open_positions: account.open_positions,
                current_leverage: account.current_leverage,
            },
            risk: RiskCard {
                kill_switch_manual: account.kill_switch_manual,
                drawdown_pct,
                current_leverage: account.current_leverage,
                day_pnl: account.day_pnl,
            },
            open_positions,
            equity_curve,
        }
    }
}

fn side_label(side: Option<qtss_domain::v2::intent::Side>) -> String {
    // Single dispatch — keeps every wire-side string in one place
    // (CLAUDE.md rule #1) so the frontend never needs a second source.
    match side {
        Some(qtss_domain::v2::intent::Side::Long) => "long".into(),
        Some(qtss_domain::v2::intent::Side::Short) => "short".into(),
        None => "flat".into(),
    }
}

fn compute_drawdown_pct(account: &AccountState) -> Decimal {
    if account.peak_equity <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let dd = account.peak_equity - account.equity;
    if dd <= Decimal::ZERO {
        Decimal::ZERO
    } else {
        dd / account.peak_equity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qtss_domain::v2::intent::Side;
    use qtss_portfolio::PortfolioConfig;
    use rust_decimal_macros::dec;

    fn engine() -> PortfolioEngine {
        let mut e = PortfolioEngine::new(PortfolioConfig {
            starting_equity: dec!(10000),
        });
        e.apply_fill("BTCUSDT", Side::Long, dec!(0.1), dec!(50000), dec!(2));
        e.mark("BTCUSDT", dec!(51000));
        e
    }

    #[test]
    fn snapshot_picks_open_positions() {
        let e = engine();
        let acc = e.snapshot();
        let snap = DashboardSnapshot::build(&e, &acc, vec![]);
        assert_eq!(snap.open_positions.len(), 1);
        assert_eq!(snap.open_positions[0].symbol, "BTCUSDT");
        assert_eq!(snap.open_positions[0].side, "long");
    }

    #[test]
    fn drawdown_reflects_peak_minus_equity() {
        let e = engine();
        let acc = e.snapshot();
        let snap = DashboardSnapshot::build(&e, &acc, vec![]);
        // Peak = starting equity (10000); current equity ≈ 5098 after
        // the buy + mark, so drawdown ≈ 49%.
        assert!(snap.risk.drawdown_pct > dec!(0));
        assert!(snap.risk.drawdown_pct < dec!(1));
    }

    #[test]
    fn json_round_trip() {
        let e = engine();
        let acc = e.snapshot();
        let snap = DashboardSnapshot::build(
            &e,
            &acc,
            vec![EquityPoint { at: Utc::now(), equity: dec!(10000) }],
        );
        let j = serde_json::to_string(&snap).unwrap();
        let back: DashboardSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back.open_positions.len(), 1);
    }
}
