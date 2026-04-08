//! TradeIntent and OrderRequest — the bridge between analysis and execution.
//!
//! Flow (see plan §7B):
//!     SignalEnvelope --(strategy)--> TradeIntent --(risk)--> ApprovedIntent
//!         --(execution)--> OrderRequest --(broker adapter)--> venue
//!
//! Strategies emit `TradeIntent` (high-level: where to enter, where to exit,
//! how much risk). Execution adapts that into venue-agnostic `OrderRequest`s.
//! Risk lives between the two — pre-trade checks, sizing, kill-switch.

use crate::execution::ExecutionMode;
use crate::v2::detection::Target;
use crate::v2::instrument::Instrument;
use crate::v2::timeframe::Timeframe;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Single, stable side enum used across v2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Long,
    Short,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Market,
    Limit,
    Stop,
    StopLimit,
    Oco,
    Iceberg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeInForce {
    Gtc,
    Ioc,
    Fok,
    Day,
    Gtd,
}

/// How a strategy wants the position sized. Concrete value is computed
/// by `qtss-risk` after looking up the relevant config keys; the strategy
/// only declares intent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SizingHint {
    /// Risk N% of equity over the stop distance.
    RiskPct { pct: Decimal },
    /// Quarter-Kelly etc; fraction is config-driven, this is just a marker.
    Kelly,
    /// Fixed notional in quote currency.
    FixedNotional { notional: Decimal },
    /// Volatility-target sizing (ATR-based).
    VolTarget,
}

/// Strategy output — what the strategy *wants* to happen. The risk layer
/// approves/rejects/resizes; never bypassed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeIntent {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub strategy_id: String,
    pub instrument: Instrument,
    pub timeframe: Timeframe,
    pub side: Side,
    pub sizing: SizingHint,
    /// Desired entry. `None` for market entry at next bar.
    pub entry_price: Option<Decimal>,
    pub stop_loss: Decimal,
    pub take_profits: Vec<Target>,
    pub time_in_force: TimeInForce,
    /// Optional time stop in seconds.
    pub time_stop_secs: Option<i64>,
    /// IDs of detections / signals that produced this intent (audit + attribution).
    pub source_signals: Vec<Uuid>,
    /// Strategy's own conviction 0..1 — separate from validator confidence.
    pub conviction: f32,
    /// Which run mode this intent was produced in.
    pub mode: RunMode,
}

/// Venue-agnostic order request handed to a broker adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderRequest {
    pub client_order_id: Uuid,
    pub instrument: Instrument,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    /// Required for Limit / Stop / StopLimit.
    pub price: Option<Decimal>,
    /// Stop trigger price for Stop / StopLimit.
    pub stop_price: Option<Decimal>,
    pub time_in_force: TimeInForce,
    pub reduce_only: bool,
    pub post_only: bool,
    /// Link back to the originating intent for audit.
    pub intent_id: Option<Uuid>,
}

/// Re-export of `ExecutionMode` under the v2 name `RunMode`. Same values,
/// same wire format — kept as an alias so v2 docs and code use a single
/// term ("run mode") without breaking existing imports of `ExecutionMode`.
pub type RunMode = ExecutionMode;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::instrument::{AssetClass, SessionCalendar, Venue};
    use rust_decimal_macros::dec;

    fn instrument() -> Instrument {
        Instrument {
            venue: Venue::Binance,
            asset_class: AssetClass::CryptoSpot,
            symbol: "BTCUSDT".into(),
            quote_ccy: "USDT".into(),
            tick_size: dec!(0.01),
            lot_size: dec!(0.00001),
            session: SessionCalendar::binance_24x7(),
        }
    }

    #[test]
    fn trade_intent_round_trip() {
        let intent = TradeIntent {
            id: Uuid::new_v4(),
            created_at: Utc::now(),
            strategy_id: "trend_follow_v1".into(),
            instrument: instrument(),
            timeframe: Timeframe::H4,
            side: Side::Long,
            sizing: SizingHint::RiskPct { pct: dec!(0.5) },
            entry_price: Some(dec!(100.0)),
            stop_loss: dec!(95.0),
            take_profits: vec![],
            time_in_force: TimeInForce::Gtc,
            time_stop_secs: Some(86_400),
            source_signals: vec![Uuid::new_v4()],
            conviction: 0.78,
            mode: RunMode::Dry,
        };
        let j = serde_json::to_string(&intent).unwrap();
        let back: TradeIntent = serde_json::from_str(&j).unwrap();
        assert_eq!(intent, back);
    }

    #[test]
    fn order_request_round_trip() {
        let req = OrderRequest {
            client_order_id: Uuid::new_v4(),
            instrument: instrument(),
            side: Side::Long,
            order_type: OrderType::Limit,
            quantity: dec!(0.01),
            price: Some(dec!(99.5)),
            stop_price: None,
            time_in_force: TimeInForce::Gtc,
            reduce_only: false,
            post_only: true,
            intent_id: None,
        };
        let j = serde_json::to_string(&req).unwrap();
        let back: OrderRequest = serde_json::from_str(&j).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn run_mode_aliases_execution_mode_wire_format() {
        let m: RunMode = RunMode::Backtest;
        let j = serde_json::to_string(&m).unwrap();
        assert_eq!(j, "\"backtest\"");
    }
}
