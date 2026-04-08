//! v2 reconciliation — venue-agnostic snapshot diff.
//!
//! Inputs are deliberately simple data structs. Adapters (Binance,
//! Alpaca, IBKR…) build the [`BrokerSnapshot`] from their own REST
//! responses; the portfolio engine produces the [`EngineSnapshot`].
//! [`reconcile`] is a pure function — no IO, no clock — so it tests
//! cleanly and the same code runs in live, dry and backtest modes.
//!
//! Drift is split into two categories:
//!
//! - **Position drift**: net quantity / side mismatch on a symbol.
//! - **Order drift**: an open order known to one side but not the
//!   other (broker-only ⇒ rogue / out-of-band entry; engine-only ⇒
//!   our placement that the venue rejected silently).
//!
//! Callers decide what to do with the report. Typical responses:
//! page the on-call when `severity == Critical`, auto-close stragglers
//! when `severity == Drift`, log otherwise.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One position as the broker reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerPosition {
    pub symbol: String,
    /// Signed net quantity (positive = long, negative = short).
    pub net_qty: Decimal,
    pub avg_entry: Decimal,
}

/// One position as the engine believes it holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnginePosition {
    pub symbol: String,
    pub net_qty: Decimal,
    pub avg_entry: Decimal,
}

/// One open order as the broker reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerOpenOrder {
    /// Client order id (UUID string) — primary correlation key.
    pub client_order_id: String,
    pub symbol: String,
    pub remaining_qty: Decimal,
}

/// One open order the engine has on its books.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineOpenOrder {
    pub client_order_id: String,
    pub symbol: String,
    pub remaining_qty: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerSnapshot {
    pub venue: String,
    pub positions: Vec<BrokerPosition>,
    pub open_orders: Vec<BrokerOpenOrder>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineSnapshot {
    pub venue: String,
    pub positions: Vec<EnginePosition>,
    pub open_orders: Vec<EngineOpenOrder>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftSeverity {
    /// Snapshots agree.
    None,
    /// Quantitative drift inside tolerance — log only.
    Soft,
    /// Quantitative drift outside tolerance — needs action.
    Drift,
    /// Position exists on one side and not the other entirely.
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionDrift {
    pub symbol: String,
    pub broker_qty: Decimal,
    pub engine_qty: Decimal,
    pub abs_delta: Decimal,
    pub severity: DriftSeverity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderDrift {
    pub client_order_id: String,
    pub symbol: String,
    /// Where the order was found.
    pub side: OrderDriftSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderDriftSide {
    /// Broker has it, engine does not (out-of-band entry / stale ack).
    BrokerOnly,
    /// Engine has it, broker does not (silent reject / lost in flight).
    EngineOnly,
    /// Both sides have it but `remaining_qty` disagrees.
    QuantityMismatch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub venue: String,
    pub positions: Vec<PositionDrift>,
    pub orders: Vec<OrderDrift>,
    pub overall: DriftSeverity,
}

impl ReconcileReport {
    pub fn is_clean(&self) -> bool {
        matches!(self.overall, DriftSeverity::None)
    }
}

/// Tolerances. Pulled from `qtss_config` by the caller — no defaults
/// in this crate (CLAUDE.md rule #2).
#[derive(Debug, Clone, Copy)]
pub struct ReconcileTolerance {
    /// Absolute quantity delta below which a position drift is `Soft`.
    pub position_qty_eps: Decimal,
}

/// Diff broker truth against engine state. Pure function.
pub fn reconcile(
    broker: &BrokerSnapshot,
    engine: &EngineSnapshot,
    tol: ReconcileTolerance,
) -> ReconcileReport {
    debug_assert_eq!(broker.venue, engine.venue, "venue mismatch");
    let positions = diff_positions(&broker.positions, &engine.positions, tol);
    let orders = diff_orders(&broker.open_orders, &engine.open_orders);
    let overall = roll_up(&positions, &orders);
    ReconcileReport {
        venue: broker.venue.clone(),
        positions,
        orders,
        overall,
    }
}

fn diff_positions(
    bp: &[BrokerPosition],
    ep: &[EnginePosition],
    tol: ReconcileTolerance,
) -> Vec<PositionDrift> {
    let broker: BTreeMap<&str, Decimal> =
        bp.iter().map(|p| (p.symbol.as_str(), p.net_qty)).collect();
    let engine: BTreeMap<&str, Decimal> =
        ep.iter().map(|p| (p.symbol.as_str(), p.net_qty)).collect();
    let symbols: BTreeSet<&str> = broker.keys().chain(engine.keys()).copied().collect();

    symbols
        .into_iter()
        .filter_map(|sym| {
            let b = broker.get(sym).copied().unwrap_or(Decimal::ZERO);
            let e = engine.get(sym).copied().unwrap_or(Decimal::ZERO);
            let delta = (b - e).abs();
            let severity = classify_position(b, e, delta, tol);
            if matches!(severity, DriftSeverity::None) {
                None
            } else {
                Some(PositionDrift {
                    symbol: sym.to_string(),
                    broker_qty: b,
                    engine_qty: e,
                    abs_delta: delta,
                    severity,
                })
            }
        })
        .collect()
}

fn classify_position(
    broker: Decimal,
    engine: Decimal,
    delta: Decimal,
    tol: ReconcileTolerance,
) -> DriftSeverity {
    // One side flat, the other not → Critical (rogue position).
    let broker_flat = broker == Decimal::ZERO;
    let engine_flat = engine == Decimal::ZERO;
    if broker_flat ^ engine_flat {
        return DriftSeverity::Critical;
    }
    if delta == Decimal::ZERO {
        return DriftSeverity::None;
    }
    // Sign flip is always critical regardless of magnitude.
    if (broker > Decimal::ZERO) != (engine > Decimal::ZERO) {
        return DriftSeverity::Critical;
    }
    if delta <= tol.position_qty_eps {
        DriftSeverity::Soft
    } else {
        DriftSeverity::Drift
    }
}

fn diff_orders(
    bo: &[BrokerOpenOrder],
    eo: &[EngineOpenOrder],
) -> Vec<OrderDrift> {
    let broker: BTreeMap<&str, &BrokerOpenOrder> =
        bo.iter().map(|o| (o.client_order_id.as_str(), o)).collect();
    let engine: BTreeMap<&str, &EngineOpenOrder> =
        eo.iter().map(|o| (o.client_order_id.as_str(), o)).collect();

    let mut out = Vec::new();
    for (id, b) in &broker {
        match engine.get(id) {
            None => out.push(OrderDrift {
                client_order_id: id.to_string(),
                symbol: b.symbol.clone(),
                side: OrderDriftSide::BrokerOnly,
            }),
            Some(e) if e.remaining_qty != b.remaining_qty => out.push(OrderDrift {
                client_order_id: id.to_string(),
                symbol: b.symbol.clone(),
                side: OrderDriftSide::QuantityMismatch,
            }),
            _ => {}
        }
    }
    for (id, e) in &engine {
        if !broker.contains_key(id) {
            out.push(OrderDrift {
                client_order_id: id.to_string(),
                symbol: e.symbol.clone(),
                side: OrderDriftSide::EngineOnly,
            });
        }
    }
    out
}

fn roll_up(positions: &[PositionDrift], orders: &[OrderDrift]) -> DriftSeverity {
    let mut worst = DriftSeverity::None;
    for p in positions {
        worst = max_sev(worst, p.severity);
    }
    if !orders.is_empty() {
        // Any open-order drift is at minimum Drift; the caller decides
        // whether to page on it.
        worst = max_sev(worst, DriftSeverity::Drift);
    }
    worst
}

fn max_sev(a: DriftSeverity, b: DriftSeverity) -> DriftSeverity {
    fn rank(s: DriftSeverity) -> u8 {
        match s {
            DriftSeverity::None => 0,
            DriftSeverity::Soft => 1,
            DriftSeverity::Drift => 2,
            DriftSeverity::Critical => 3,
        }
    }
    if rank(a) >= rank(b) {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn tol() -> ReconcileTolerance {
        ReconcileTolerance { position_qty_eps: dec!(0.0001) }
    }

    fn engine(venue: &str) -> EngineSnapshot {
        EngineSnapshot {
            venue: venue.into(),
            positions: vec![],
            open_orders: vec![],
        }
    }

    fn broker(venue: &str) -> BrokerSnapshot {
        BrokerSnapshot {
            venue: venue.into(),
            positions: vec![],
            open_orders: vec![],
        }
    }

    #[test]
    fn empty_snapshots_are_clean() {
        let r = reconcile(&broker("binance"), &engine("binance"), tol());
        assert!(r.is_clean());
    }

    #[test]
    fn matching_position_is_clean() {
        let mut b = broker("binance");
        let mut e = engine("binance");
        b.positions.push(BrokerPosition {
            symbol: "BTCUSDT".into(),
            net_qty: dec!(0.5),
            avg_entry: dec!(50000),
        });
        e.positions.push(EnginePosition {
            symbol: "BTCUSDT".into(),
            net_qty: dec!(0.5),
            avg_entry: dec!(50000),
        });
        let r = reconcile(&b, &e, tol());
        assert!(r.is_clean());
    }

    #[test]
    fn engine_missing_position_is_critical() {
        let mut b = broker("binance");
        let e = engine("binance");
        b.positions.push(BrokerPosition {
            symbol: "BTCUSDT".into(),
            net_qty: dec!(0.5),
            avg_entry: dec!(50000),
        });
        let r = reconcile(&b, &e, tol());
        assert_eq!(r.overall, DriftSeverity::Critical);
        assert_eq!(r.positions.len(), 1);
        assert_eq!(r.positions[0].severity, DriftSeverity::Critical);
    }

    #[test]
    fn small_quantity_drift_is_soft() {
        let mut b = broker("binance");
        let mut e = engine("binance");
        b.positions.push(BrokerPosition {
            symbol: "BTCUSDT".into(),
            net_qty: dec!(0.50005),
            avg_entry: dec!(50000),
        });
        e.positions.push(EnginePosition {
            symbol: "BTCUSDT".into(),
            net_qty: dec!(0.50000),
            avg_entry: dec!(50000),
        });
        let r = reconcile(&b, &e, tol());
        assert_eq!(r.positions[0].severity, DriftSeverity::Soft);
    }

    #[test]
    fn sign_flip_is_critical() {
        let mut b = broker("binance");
        let mut e = engine("binance");
        b.positions.push(BrokerPosition {
            symbol: "BTCUSDT".into(),
            net_qty: dec!(0.5),
            avg_entry: dec!(50000),
        });
        e.positions.push(EnginePosition {
            symbol: "BTCUSDT".into(),
            net_qty: dec!(-0.5),
            avg_entry: dec!(50000),
        });
        let r = reconcile(&b, &e, tol());
        assert_eq!(r.positions[0].severity, DriftSeverity::Critical);
    }

    #[test]
    fn broker_only_order_flagged() {
        let mut b = broker("binance");
        let e = engine("binance");
        b.open_orders.push(BrokerOpenOrder {
            client_order_id: "abc".into(),
            symbol: "BTCUSDT".into(),
            remaining_qty: dec!(0.1),
        });
        let r = reconcile(&b, &e, tol());
        assert_eq!(r.orders.len(), 1);
        assert_eq!(r.orders[0].side, OrderDriftSide::BrokerOnly);
        assert_eq!(r.overall, DriftSeverity::Drift);
    }

    #[test]
    fn quantity_mismatch_on_known_order() {
        let mut b = broker("binance");
        let mut e = engine("binance");
        b.open_orders.push(BrokerOpenOrder {
            client_order_id: "abc".into(),
            symbol: "BTCUSDT".into(),
            remaining_qty: dec!(0.1),
        });
        e.open_orders.push(EngineOpenOrder {
            client_order_id: "abc".into(),
            symbol: "BTCUSDT".into(),
            remaining_qty: dec!(0.2),
        });
        let r = reconcile(&b, &e, tol());
        assert_eq!(r.orders[0].side, OrderDriftSide::QuantityMismatch);
    }
}
