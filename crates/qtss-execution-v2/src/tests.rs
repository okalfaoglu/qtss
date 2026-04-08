use crate::adapter::OrderStatus;
use crate::builder::split_intent;
use crate::router::ExecutionRouter;
use crate::sim::{SimAdapter, SimConfig};
use chrono::Utc;
use qtss_domain::execution::ExecutionMode;
use qtss_domain::v2::detection::{Target, TargetMethod};
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::intent::{
    ApprovedIntent, OrderType, Side, SizingHint, TimeInForce, TradeIntent,
};
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;
use uuid::Uuid;

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

fn intent(side: Side, entry: Option<Decimal>, tps: Vec<Target>) -> TradeIntent {
    TradeIntent {
        id: Uuid::new_v4(),
        created_at: Utc::now(),
        strategy_id: "test".into(),
        instrument: instrument(),
        timeframe: Timeframe::H4,
        side,
        sizing: SizingHint::RiskPct { pct: dec!(0.005) },
        entry_price: entry,
        stop_loss: dec!(95),
        take_profits: tps,
        time_in_force: TimeInForce::Gtc,
        time_stop_secs: None,
        source_signals: vec![],
        conviction: 0.7,
        mode: ExecutionMode::Dry,
    }
}

fn approved(intent: TradeIntent, qty: Decimal) -> ApprovedIntent {
    let entry = intent.entry_price.unwrap_or(dec!(100));
    ApprovedIntent {
        id: Uuid::new_v4(),
        approved_at: Utc::now(),
        notional: qty * entry,
        intent,
        quantity: qty,
        checks_passed: vec![],
        adjustments: vec![],
    }
}

fn target(price: Decimal, weight: f32) -> Target {
    Target {
        price,
        method: TargetMethod::FibExtension,
        weight,
        label: None,
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

#[test]
fn builder_emits_entry_stop_and_two_tps() {
    let intent = intent(
        Side::Long,
        Some(dec!(100)),
        vec![target(dec!(110), 0.6), target(dec!(120), 0.4)],
    );
    let approved = approved(intent, dec!(1));
    let bracket = split_intent(&approved).unwrap();
    assert_eq!(bracket.entry.order_type, OrderType::Limit);
    assert_eq!(bracket.entry.side, Side::Long);
    assert_eq!(bracket.entry.quantity, dec!(1));
    assert_eq!(bracket.stop.order_type, OrderType::Stop);
    assert_eq!(bracket.stop.side, Side::Short);
    assert!(bracket.stop.reduce_only);
    assert_eq!(bracket.take_profits.len(), 2);
    assert!(bracket.take_profits.iter().all(|t| t.reduce_only));
    // children sum to parent
    let sum: Decimal = bracket.take_profits.iter().map(|t| t.quantity).sum();
    assert_eq!(sum, dec!(1));
}

#[test]
fn builder_market_when_no_entry_price() {
    let intent = intent(Side::Long, None, vec![]);
    let approved = approved(intent, dec!(0.5));
    let bracket = split_intent(&approved).unwrap();
    assert_eq!(bracket.entry.order_type, OrderType::Market);
    assert!(bracket.entry.price.is_none());
}

#[test]
fn builder_short_flips_exit_sides() {
    let intent = intent(
        Side::Short,
        Some(dec!(100)),
        vec![target(dec!(90), 1.0)],
    );
    let approved = approved(intent, dec!(2));
    let bracket = split_intent(&approved).unwrap();
    assert_eq!(bracket.entry.side, Side::Short);
    assert_eq!(bracket.stop.side, Side::Long);
    assert_eq!(bracket.take_profits[0].side, Side::Long);
}

#[test]
fn builder_rejects_zero_quantity() {
    let intent = intent(Side::Long, Some(dec!(100)), vec![]);
    let approved = approved(intent, dec!(0));
    assert!(split_intent(&approved).is_err());
}

#[test]
fn builder_equal_split_when_all_weights_zero() {
    let intent = intent(
        Side::Long,
        Some(dec!(100)),
        vec![target(dec!(110), 0.0), target(dec!(120), 0.0)],
    );
    let approved = approved(intent, dec!(2));
    let bracket = split_intent(&approved).unwrap();
    assert_eq!(bracket.take_profits[0].quantity, dec!(1));
    assert_eq!(bracket.take_profits[1].quantity, dec!(1));
}

// ---------------------------------------------------------------------------
// SimAdapter
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sim_fills_market_at_reference_with_slippage() {
    use crate::adapter::ExecutionAdapter;
    let sim = SimAdapter::new(SimConfig::defaults());
    sim.set_reference_price(dec!(100));
    let intent = intent(Side::Long, None, vec![]);
    let approved = approved(intent, dec!(1));
    let bracket = split_intent(&approved).unwrap();
    let ack = sim.place(bracket.entry).await.unwrap();
    assert_eq!(ack.status, OrderStatus::Filled);
    assert_eq!(ack.fills.len(), 1);
    let fp = ack.fills[0].price;
    // slip 0.0005 long → 100 + 0.05 = 100.05
    assert_eq!(fp, dec!(100.0500));
}

#[tokio::test]
async fn sim_fills_limit_at_stated_price() {
    use crate::adapter::ExecutionAdapter;
    let sim = SimAdapter::new(SimConfig::defaults());
    let intent = intent(Side::Long, Some(dec!(99.5)), vec![]);
    let approved = approved(intent, dec!(1));
    let bracket = split_intent(&approved).unwrap();
    let ack = sim.place(bracket.entry).await.unwrap();
    assert_eq!(ack.fills[0].price, dec!(99.5));
}

#[tokio::test]
async fn sim_status_returns_known_order() {
    use crate::adapter::ExecutionAdapter;
    let sim = SimAdapter::new(SimConfig::defaults());
    let intent = intent(Side::Long, Some(dec!(99.5)), vec![]);
    let approved = approved(intent, dec!(1));
    let bracket = split_intent(&approved).unwrap();
    let ack = sim.place(bracket.entry).await.unwrap();
    let again = sim.status(ack.client_order_id).await.unwrap();
    assert_eq!(again.client_order_id, ack.client_order_id);
}

#[tokio::test]
async fn sim_status_unknown_order_errors() {
    use crate::adapter::ExecutionAdapter;
    let sim = SimAdapter::new(SimConfig::defaults());
    assert!(sim.status(Uuid::new_v4()).await.is_err());
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

#[tokio::test]
async fn router_places_full_bracket_via_dry_adapter() {
    let sim = Arc::new(SimAdapter::new(SimConfig::defaults()));
    sim.set_reference_price(dec!(100));
    let mut router = ExecutionRouter::new();
    router.register(ExecutionMode::Dry, sim.clone());
    let intent = intent(
        Side::Long,
        Some(dec!(100)),
        vec![target(dec!(110), 0.6), target(dec!(120), 0.4)],
    );
    let approved = approved(intent, dec!(1));
    let acks = router.route(&approved).await.unwrap();
    assert_eq!(acks.entry.status, OrderStatus::Filled);
    assert_eq!(acks.stop.status, OrderStatus::Filled);
    assert_eq!(acks.take_profits.len(), 2);
}

#[tokio::test]
async fn router_errors_when_mode_not_registered() {
    let router = ExecutionRouter::new();
    let intent = intent(Side::Long, Some(dec!(100)), vec![]);
    let approved = approved(intent, dec!(1));
    assert!(router.route(&approved).await.is_err());
}

#[tokio::test]
async fn router_adapter_count_tracks_registrations() {
    let mut router = ExecutionRouter::new();
    assert_eq!(router.adapter_count(), 0);
    router.register(
        ExecutionMode::Dry,
        Arc::new(SimAdapter::new(SimConfig::defaults())),
    );
    assert_eq!(router.adapter_count(), 1);
}
