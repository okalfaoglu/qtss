use crate::checks::{
    DailyLossCheck, DrawdownCheck, KillSwitchCheck, LeverageCheck, MaxOpenPositionsCheck,
    RiskCheck, StopDistanceCheck,
};
use crate::config::RiskConfig;
use crate::engine::RiskEngine;
use crate::sizing::SizerRegistry;
use crate::state::AccountState;
use chrono::Utc;
use qtss_domain::execution::ExecutionMode;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::intent::{
    RiskRejection, Side, SizingHint, TimeInForce, TradeIntent,
};
use qtss_domain::v2::timeframe::Timeframe;
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

fn account(equity: rust_decimal::Decimal) -> AccountState {
    AccountState {
        equity,
        peak_equity: equity,
        day_pnl: dec!(0),
        open_positions: 0,
        current_leverage: dec!(0),
        kill_switch_manual: false,
    }
}

fn intent_with(sizing: SizingHint, entry: rust_decimal::Decimal, stop: rust_decimal::Decimal) -> TradeIntent {
    TradeIntent {
        id: Uuid::new_v4(),
        created_at: Utc::now(),
        strategy_id: "test".into(),
        instrument: instrument(),
        timeframe: Timeframe::H4,
        side: Side::Long,
        sizing,
        entry_price: Some(entry),
        stop_loss: stop,
        take_profits: vec![],
        time_in_force: TimeInForce::Gtc,
        time_stop_secs: None,
        source_signals: vec![],
        conviction: 0.7,
        mode: ExecutionMode::Dry,
    }
}

fn full_engine() -> RiskEngine {
    let mut e = RiskEngine::new(RiskConfig::defaults(), SizerRegistry::with_defaults()).unwrap();
    e.register_check(Arc::new(KillSwitchCheck));
    e.register_check(Arc::new(DrawdownCheck));
    e.register_check(Arc::new(DailyLossCheck));
    e.register_check(Arc::new(MaxOpenPositionsCheck));
    e.register_check(Arc::new(LeverageCheck));
    e.register_check(Arc::new(StopDistanceCheck));
    e
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[test]
fn config_defaults_validate() {
    RiskConfig::defaults().validate().unwrap();
}

#[test]
fn config_rejects_max_dd_above_killswitch() {
    let mut c = RiskConfig::defaults();
    c.max_drawdown = dec!(0.20);
    assert!(c.validate().is_err());
}

#[test]
fn config_rejects_zero_open_positions() {
    let mut c = RiskConfig::defaults();
    c.max_open_positions = 0;
    assert!(c.validate().is_err());
}

// ---------------------------------------------------------------------------
// AccountState helpers
// ---------------------------------------------------------------------------

#[test]
fn account_drawdown_basic() {
    let s = AccountState {
        equity: dec!(950),
        peak_equity: dec!(1000),
        day_pnl: dec!(0),
        open_positions: 0,
        current_leverage: dec!(0),
        kill_switch_manual: false,
    };
    assert_eq!(s.drawdown(), dec!(0.05));
}

#[test]
fn account_day_loss_zero_when_in_profit() {
    let mut s = account(dec!(1000));
    s.day_pnl = dec!(50);
    assert_eq!(s.day_loss_pct(), dec!(0));
}

// ---------------------------------------------------------------------------
// Individual checks
// ---------------------------------------------------------------------------

#[test]
fn killswitch_blocks_when_manual_flag_set() {
    let cfg = RiskConfig::defaults();
    let mut s = account(dec!(1000));
    s.kill_switch_manual = true;
    let intent = intent_with(SizingHint::RiskPct { pct: dec!(0.005) }, dec!(100), dec!(95));
    let r = KillSwitchCheck.evaluate(&intent, &s, &cfg);
    assert!(matches!(r, Err(RiskRejection::KillSwitchActive(_))));
}

#[test]
fn killswitch_blocks_on_drawdown_breach() {
    let cfg = RiskConfig::defaults();
    let s = AccountState {
        equity: dec!(900),
        peak_equity: dec!(1000),
        day_pnl: dec!(0),
        open_positions: 0,
        current_leverage: dec!(0),
        kill_switch_manual: false,
    };
    let intent = intent_with(SizingHint::RiskPct { pct: dec!(0.005) }, dec!(100), dec!(95));
    let r = KillSwitchCheck.evaluate(&intent, &s, &cfg);
    assert!(matches!(r, Err(RiskRejection::KillSwitchActive(_))));
}

#[test]
fn max_open_positions_blocks_at_cap() {
    let cfg = RiskConfig::defaults();
    let mut s = account(dec!(1000));
    s.open_positions = cfg.max_open_positions;
    let intent = intent_with(SizingHint::RiskPct { pct: dec!(0.005) }, dec!(100), dec!(95));
    let r = MaxOpenPositionsCheck.evaluate(&intent, &s, &cfg);
    assert!(matches!(r, Err(RiskRejection::MaxOpenPositionsReached { .. })));
}

#[test]
fn leverage_check_blocks_above_cap() {
    let cfg = RiskConfig::defaults();
    let mut s = account(dec!(1000));
    s.current_leverage = dec!(2.0);
    let intent = intent_with(SizingHint::RiskPct { pct: dec!(0.005) }, dec!(100), dec!(95));
    let r = LeverageCheck.evaluate(&intent, &s, &cfg);
    assert!(matches!(r, Err(RiskRejection::MaxLeverageExceeded { .. })));
}

#[test]
fn stop_distance_check_blocks_zero_distance() {
    let cfg = RiskConfig::defaults();
    let s = account(dec!(1000));
    let intent = intent_with(SizingHint::RiskPct { pct: dec!(0.005) }, dec!(100), dec!(100));
    let r = StopDistanceCheck.evaluate(&intent, &s, &cfg);
    assert!(matches!(r, Err(RiskRejection::StopDistanceTooSmall)));
}

// ---------------------------------------------------------------------------
// Engine — happy path
// ---------------------------------------------------------------------------

#[test]
fn engine_approves_normal_intent() {
    let e = full_engine();
    let s = account(dec!(10000));
    let intent = intent_with(SizingHint::RiskPct { pct: dec!(0.005) }, dec!(100), dec!(95));
    let approved = e.approve(intent, &s).expect("should approve");
    // 0.5% of 10000 = 50 quote risk; distance = 5 → qty = 10
    assert_eq!(approved.quantity, dec!(10));
    assert_eq!(approved.notional, dec!(1000));
    assert!(approved.checks_passed.contains(&"kill_switch".to_string()));
    assert!(approved.checks_passed.contains(&"stop_distance".to_string()));
    assert!(approved.adjustments.is_empty());
}

#[test]
fn engine_trims_quantity_for_per_trade_risk_cap() {
    let e = full_engine();
    let s = account(dec!(10000));
    // Strategy asks for 5% per trade; cap is 1% — sizer should trim.
    let intent = intent_with(SizingHint::RiskPct { pct: dec!(0.05) }, dec!(100), dec!(95));
    let approved = e.approve(intent, &s).unwrap();
    // 1% of 10000 = 100 quote risk; distance = 5 → qty = 20
    assert_eq!(approved.quantity, dec!(20));
    assert!(approved
        .adjustments
        .iter()
        .any(|a| a.contains("risk_pct trimmed")));
}

#[test]
fn engine_trims_quantity_for_leverage_cap() {
    let e = full_engine();
    let mut s = account(dec!(1000));
    s.current_leverage = dec!(0); // current is fine, leverage check passes
    // Force a huge sized notional via FixedNotional (5x equity).
    let intent = intent_with(
        SizingHint::FixedNotional {
            notional: dec!(5000),
        },
        dec!(100),
        dec!(95),
    );
    let approved = e.approve(intent, &s).unwrap();
    // Sized would be 50 units * 100 = 5000 notional; cap = 1000.
    assert_eq!(approved.notional, dec!(1000));
    assert_eq!(approved.quantity, dec!(10));
    assert!(approved
        .adjustments
        .iter()
        .any(|a| a.contains("max_leverage")));
}

// ---------------------------------------------------------------------------
// Engine — rejection paths
// ---------------------------------------------------------------------------

#[test]
fn engine_rejects_when_killswitch_set() {
    let e = full_engine();
    let mut s = account(dec!(10000));
    s.kill_switch_manual = true;
    let intent = intent_with(SizingHint::RiskPct { pct: dec!(0.005) }, dec!(100), dec!(95));
    assert!(matches!(
        e.approve(intent, &s),
        Err(RiskRejection::KillSwitchActive(_))
    ));
}

#[test]
fn engine_rejects_when_open_positions_at_cap() {
    let e = full_engine();
    let mut s = account(dec!(10000));
    s.open_positions = RiskConfig::defaults().max_open_positions;
    let intent = intent_with(SizingHint::RiskPct { pct: dec!(0.005) }, dec!(100), dec!(95));
    assert!(matches!(
        e.approve(intent, &s),
        Err(RiskRejection::MaxOpenPositionsReached { .. })
    ));
}

#[test]
fn engine_rejects_when_no_sizer_registered() {
    let mut e = RiskEngine::new(RiskConfig::defaults(), SizerRegistry::new()).unwrap();
    e.register_check(Arc::new(KillSwitchCheck));
    let s = account(dec!(10000));
    let intent = intent_with(SizingHint::RiskPct { pct: dec!(0.005) }, dec!(100), dec!(95));
    assert!(matches!(
        e.approve(intent, &s),
        Err(RiskRejection::InvalidIntent(_))
    ));
}

#[test]
fn engine_check_count_tracks_registrations() {
    let mut e = RiskEngine::new(RiskConfig::defaults(), SizerRegistry::with_defaults()).unwrap();
    assert_eq!(e.check_count(), 0);
    e.register_check(Arc::new(KillSwitchCheck));
    assert_eq!(e.check_count(), 1);
}

#[test]
fn sizer_registry_has_four_defaults() {
    let r = SizerRegistry::with_defaults();
    assert_eq!(r.len(), 4);
    assert!(r.get("risk_pct").is_some());
    assert!(r.get("kelly").is_some());
}
