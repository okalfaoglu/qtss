use crate::config::TargetEngineConfig;
use crate::engine::TargetEngine;
use crate::methods::{
    direction_of, Direction, FibExtensionMethod, HarmonicRetracementMethod, MeasuredMoveMethod,
    TargetMethodCalc, WyckoffRangeMethod,
};
use chrono::Utc;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState, PivotRef, TargetMethod};
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::pivot::PivotLevel;
use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;

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

fn regime() -> RegimeSnapshot {
    RegimeSnapshot {
        at: Utc::now(),
        kind: RegimeKind::Ranging,
        trend_strength: TrendStrength::None,
        adx: dec!(18),
        bb_width: dec!(0.05),
        atr_pct: dec!(0.02),
        choppiness: dec!(60),
        confidence: 0.7,
    }
}

fn anchor(label: &str, price: Decimal) -> PivotRef {
    PivotRef {
        bar_index: 0,
        price,
        level: PivotLevel::L1,
        label: Some(label.into()),
    }
}

fn detection(kind: PatternKind, anchors: Vec<PivotRef>) -> Detection {
    Detection::new(
        instrument(),
        Timeframe::H4,
        kind,
        PatternState::Forming,
        anchors,
        0.8,
        dec!(0),
        regime(),
    )
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[test]
fn config_defaults_validate() {
    TargetEngineConfig::defaults().validate().unwrap();
}

#[test]
fn config_rejects_excessive_tolerance() {
    let mut c = TargetEngineConfig::defaults();
    c.cluster_tolerance = 0.5;
    assert!(c.validate().is_err());
}

#[test]
fn config_rejects_zero_max_targets() {
    let mut c = TargetEngineConfig::defaults();
    c.max_targets = 0;
    assert!(c.validate().is_err());
}

// ---------------------------------------------------------------------------
// direction_of
// ---------------------------------------------------------------------------

#[test]
fn direction_bull_subkinds() {
    assert_eq!(
        direction_of(&PatternKind::Elliott("impulse_5_bull".into())),
        Some(Direction::Long)
    );
    assert_eq!(
        direction_of(&PatternKind::Classical("double_bottom_bull".into())),
        Some(Direction::Long)
    );
}

#[test]
fn direction_bear_subkinds() {
    assert_eq!(
        direction_of(&PatternKind::Classical("double_top_bear".into())),
        Some(Direction::Short)
    );
    assert_eq!(
        direction_of(&PatternKind::Wyckoff("upthrust_bear".into())),
        Some(Direction::Short)
    );
}

#[test]
fn direction_neutral_returns_none() {
    assert_eq!(
        direction_of(&PatternKind::Classical("symmetrical_triangle_neutral".into())),
        None
    );
}

// ---------------------------------------------------------------------------
// MeasuredMoveMethod — double top
// ---------------------------------------------------------------------------

#[test]
fn measured_move_double_top_projects_down() {
    let det = detection(
        PatternKind::Classical("double_top_bear".into()),
        vec![
            anchor("H1", dec!(100)),
            anchor("T", dec!(80)),
            anchor("H2", dec!(101)),
        ],
    );
    let targets = MeasuredMoveMethod.project(&det);
    assert_eq!(targets.len(), 2);
    // base=80 height=20 short → t1 = 60, t2 = 80 - 32.36 ≈ 47.64
    let prices: Vec<f64> = targets
        .iter()
        .map(|t| {
            use rust_decimal::prelude::ToPrimitive;
            t.price.to_f64().unwrap()
        })
        .collect();
    assert!((prices[0] - 60.0).abs() < 0.5);
    assert!((prices[1] - 47.64).abs() < 0.5);
    assert!(targets.iter().all(|t| t.method == TargetMethod::MeasuredMove));
}

#[test]
fn measured_move_returns_empty_for_non_classical() {
    let det = detection(
        PatternKind::Harmonic("gartley_bull".into()),
        vec![anchor("X", dec!(0)), anchor("A", dec!(100))],
    );
    assert!(MeasuredMoveMethod.project(&det).is_empty());
}

// ---------------------------------------------------------------------------
// FibExtensionMethod
// ---------------------------------------------------------------------------

#[test]
fn fib_extension_elliott_bull() {
    let det = detection(
        PatternKind::Elliott("impulse_5_bull".into()),
        vec![
            anchor("0", dec!(100)),
            anchor("1", dec!(120)),
            anchor("2", dec!(110)),
            anchor("3", dec!(150)),
            anchor("4", dec!(140)),
        ],
    );
    let targets = FibExtensionMethod.project(&det);
    assert_eq!(targets.len(), 3);
    // wave1=20, p4=140 long: 160, 172.36, 192.36
    use rust_decimal::prelude::ToPrimitive;
    let p: Vec<f64> = targets.iter().map(|t| t.price.to_f64().unwrap()).collect();
    assert!((p[0] - 160.0).abs() < 0.1);
    assert!((p[1] - 172.36).abs() < 0.1);
    assert!((p[2] - 192.36).abs() < 0.1);
}

// ---------------------------------------------------------------------------
// HarmonicRetracementMethod
// ---------------------------------------------------------------------------

#[test]
fn harmonic_retracement_bull() {
    // bullish gartley d below a → retrace upward toward a
    let det = detection(
        PatternKind::Harmonic("gartley_bull".into()),
        vec![
            anchor("X", dec!(0)),
            anchor("A", dec!(100)),
            anchor("B", dec!(38)),
            anchor("C", dec!(76)),
            anchor("D", dec!(20)),
        ],
    );
    let targets = HarmonicRetracementMethod.project(&det);
    assert_eq!(targets.len(), 3);
    // leg=80 from D=20 long → 50.56, 69.44, 100
    use rust_decimal::prelude::ToPrimitive;
    let p: Vec<f64> = targets.iter().map(|t| t.price.to_f64().unwrap()).collect();
    assert!((p[0] - 50.56).abs() < 0.1);
    assert!((p[1] - 69.44).abs() < 0.1);
    assert!((p[2] - 100.0).abs() < 0.1);
}

// ---------------------------------------------------------------------------
// WyckoffRangeMethod
// ---------------------------------------------------------------------------

#[test]
fn wyckoff_spring_projects_up() {
    let det = detection(
        PatternKind::Wyckoff("spring_bull".into()),
        vec![
            anchor("P1", dec!(100)),
            anchor("P2", dec!(80)),
            anchor("P3", dec!(99)),
            anchor("P4", dec!(81)),
            anchor("P5", dec!(100)),
            anchor("Spring", dec!(78)),
        ],
    );
    let targets = WyckoffRangeMethod.project(&det);
    assert_eq!(targets.len(), 2);
    // body height = 100-80 = 20 from spring=78 long → 88, 98
    use rust_decimal::prelude::ToPrimitive;
    let p: Vec<f64> = targets.iter().map(|t| t.price.to_f64().unwrap()).collect();
    assert!((p[0] - 88.0).abs() < 0.5);
    assert!((p[1] - 98.0).abs() < 0.5);
}

// ---------------------------------------------------------------------------
// Engine + clustering
// ---------------------------------------------------------------------------

fn full_engine() -> TargetEngine {
    let mut e = TargetEngine::new(TargetEngineConfig::defaults()).unwrap();
    e.register(Arc::new(MeasuredMoveMethod));
    e.register(Arc::new(FibExtensionMethod));
    e.register(Arc::new(HarmonicRetracementMethod));
    e.register(Arc::new(WyckoffRangeMethod));
    e
}

#[test]
fn engine_emits_targets_for_double_top() {
    let e = full_engine();
    let det = detection(
        PatternKind::Classical("double_top_bear".into()),
        vec![
            anchor("H1", dec!(100)),
            anchor("T", dec!(80)),
            anchor("H2", dec!(101)),
        ],
    );
    let targets = e.project(&det);
    assert!(!targets.is_empty());
}

#[test]
fn engine_clusters_close_targets() {
    let mut e = TargetEngine::new(TargetEngineConfig::defaults()).unwrap();
    // Two methods that produce nearly-equal targets — both should
    // collapse into one cluster.
    struct A;
    struct B;
    impl TargetMethodCalc for A {
        fn name(&self) -> &'static str {
            "a"
        }
        fn project(&self, _det: &Detection) -> Vec<qtss_domain::v2::detection::Target> {
            vec![qtss_domain::v2::detection::Target {
                price: dec!(100.00),
                method: TargetMethod::MeasuredMove,
                weight: 0.7,
                label: Some("a".into()),
            }]
        }
    }
    impl TargetMethodCalc for B {
        fn name(&self) -> &'static str {
            "b"
        }
        fn project(&self, _det: &Detection) -> Vec<qtss_domain::v2::detection::Target> {
            vec![qtss_domain::v2::detection::Target {
                price: dec!(100.20),
                method: TargetMethod::FibExtension,
                weight: 0.6,
                label: Some("b".into()),
            }]
        }
    }
    e.register(Arc::new(A));
    e.register(Arc::new(B));
    let det = detection(
        PatternKind::Custom("any".into()),
        vec![anchor("X", dec!(50))],
    );
    let targets = e.project(&det);
    assert_eq!(targets.len(), 1, "expected cluster, got {targets:?}");
    assert_eq!(targets[0].method, TargetMethod::Cluster);
    use rust_decimal::prelude::ToPrimitive;
    let p = targets[0].price.to_f64().unwrap();
    assert!(p > 100.0 && p < 100.2);
}

#[test]
fn engine_trims_to_max_targets() {
    let mut cfg = TargetEngineConfig::defaults();
    cfg.max_targets = 2;
    let mut e = TargetEngine::new(cfg).unwrap();
    e.register(Arc::new(FibExtensionMethod)); // produces 3 targets
    let det = detection(
        PatternKind::Elliott("impulse_5_bull".into()),
        vec![
            anchor("0", dec!(100)),
            anchor("1", dec!(120)),
            anchor("2", dec!(110)),
            anchor("3", dec!(150)),
            anchor("4", dec!(140)),
        ],
    );
    let targets = e.project(&det);
    assert_eq!(targets.len(), 2);
}

#[test]
fn engine_returns_empty_when_no_method_applies() {
    let e = full_engine();
    let det = detection(
        PatternKind::Custom("nothing_here".into()),
        vec![anchor("X", dec!(100))],
    );
    assert!(e.project(&det).is_empty());
}
