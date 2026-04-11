use crate::config::ClassicalConfig;
use crate::detector::ClassicalDetector;
use crate::shapes::SHAPES;
use chrono::{TimeZone, Utc};
use qtss_domain::v2::detection::PatternKind;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::pivot::{Pivot, PivotKind, PivotLevel, PivotTree};
use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal::Decimal;
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

fn regime() -> RegimeSnapshot {
    RegimeSnapshot {
        at: Utc::now(),
        kind: RegimeKind::Ranging,
        trend_strength: TrendStrength::None,
        adx: dec!(18),
        bb_width: dec!(0.06),
        atr_pct: dec!(0.02),
        choppiness: dec!(65),
        confidence: 0.7,
    }
}

fn pivot(idx: u64, price: Decimal, kind: PivotKind) -> Pivot {
    Pivot {
        bar_index: idx,
        time: Utc.timestamp_opt(1_700_000_000 + idx as i64 * 60, 0).unwrap(),
        price,
        kind,
        level: PivotLevel::L1,
        prominence: dec!(1),
        volume_at_pivot: dec!(1),
        swing_type: None,
    }
}

fn tree_from(level_pivots: Vec<Pivot>) -> PivotTree {
    PivotTree::new(vec![], level_pivots, vec![], vec![])
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[test]
fn config_defaults_validate() {
    ClassicalConfig::defaults().validate().unwrap();
}

#[test]
fn config_rejects_bad_tolerance() {
    let mut c = ClassicalConfig::defaults();
    c.equality_tolerance = 0.9;
    assert!(c.validate().is_err());
}

#[test]
fn config_rejects_zero_horizon() {
    let mut c = ClassicalConfig::defaults();
    c.apex_horizon_bars = 0;
    assert!(c.validate().is_err());
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

#[test]
fn catalog_has_expected_shapes() {
    let names: Vec<&str> = SHAPES.iter().map(|s| s.name).collect();
    for n in [
        "double_top",
        "double_bottom",
        "head_and_shoulders",
        "inverse_head_and_shoulders",
        "ascending_triangle",
        "descending_triangle",
        "symmetrical_triangle",
    ] {
        assert!(names.contains(&n), "missing {n}");
    }
}

// ---------------------------------------------------------------------------
// Detector — too few pivots
// ---------------------------------------------------------------------------

#[test]
fn detect_returns_none_on_too_few_pivots() {
    let det = ClassicalDetector::new(ClassicalConfig::defaults()).unwrap();
    let tree = tree_from(vec![pivot(0, dec!(100), PivotKind::High)]);
    assert!(det
        .detect(&tree, &instrument(), Timeframe::H4, &regime())
        .is_none());
}

// ---------------------------------------------------------------------------
// Double top / bottom
// ---------------------------------------------------------------------------

#[test]
fn detect_double_top() {
    let det = ClassicalDetector::new(ClassicalConfig::defaults()).unwrap();
    let pivots = vec![
        pivot(0, dec!(100.0), PivotKind::High),
        pivot(1, dec!(90.0), PivotKind::Low),
        pivot(2, dec!(100.5), PivotKind::High),
    ];
    let d = det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .expect("double top should be detected");
    assert_eq!(d.kind, PatternKind::Classical("double_top_bear".into()));
    assert_eq!(d.anchors.len(), 3);
    assert_eq!(d.anchors[0].label.as_deref(), Some("H1"));
    assert_eq!(d.anchors[2].label.as_deref(), Some("H2"));
}

#[test]
fn detect_double_bottom() {
    let det = ClassicalDetector::new(ClassicalConfig::defaults()).unwrap();
    let pivots = vec![
        pivot(0, dec!(50.0), PivotKind::Low),
        pivot(1, dec!(60.0), PivotKind::High),
        pivot(2, dec!(50.4), PivotKind::Low),
    ];
    let d = det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .expect("double bottom should be detected");
    assert_eq!(d.kind, PatternKind::Classical("double_bottom_bull".into()));
}

#[test]
fn detect_double_top_rejects_unequal_peaks() {
    let det = ClassicalDetector::new(ClassicalConfig::defaults()).unwrap();
    let pivots = vec![
        pivot(0, dec!(100.0), PivotKind::High),
        pivot(1, dec!(90.0), PivotKind::Low),
        pivot(2, dec!(120.0), PivotKind::High), // 20% off
    ];
    assert!(det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .is_none());
}

// ---------------------------------------------------------------------------
// Head and shoulders
// ---------------------------------------------------------------------------

#[test]
fn detect_head_and_shoulders() {
    let det = ClassicalDetector::new(ClassicalConfig::defaults()).unwrap();
    let pivots = vec![
        pivot(0, dec!(100.0), PivotKind::High), // LS
        pivot(1, dec!(90.0), PivotKind::Low),   // N1
        pivot(2, dec!(120.0), PivotKind::High), // H
        pivot(3, dec!(91.0), PivotKind::Low),   // N2
        pivot(4, dec!(100.5), PivotKind::High), // RS
    ];
    let d = det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .expect("h&s should be detected");
    assert_eq!(
        d.kind,
        PatternKind::Classical("head_and_shoulders_bear".into())
    );
    assert_eq!(d.anchors[2].label.as_deref(), Some("H"));
}

#[test]
fn detect_inverse_head_and_shoulders() {
    let det = ClassicalDetector::new(ClassicalConfig::defaults()).unwrap();
    let pivots = vec![
        pivot(0, dec!(100.0), PivotKind::Low),
        pivot(1, dec!(110.0), PivotKind::High),
        pivot(2, dec!(80.0), PivotKind::Low),
        pivot(3, dec!(110.5), PivotKind::High),
        pivot(4, dec!(100.5), PivotKind::Low),
    ];
    let d = det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .expect("inverse h&s should be detected");
    assert_eq!(
        d.kind,
        PatternKind::Classical("inverse_head_and_shoulders_bull".into())
    );
}

#[test]
fn detect_hns_rejects_when_head_not_highest() {
    let det = ClassicalDetector::new(ClassicalConfig::defaults()).unwrap();
    let pivots = vec![
        pivot(0, dec!(120.0), PivotKind::High),
        pivot(1, dec!(90.0), PivotKind::Low),
        pivot(2, dec!(110.0), PivotKind::High), // not the highest
        pivot(3, dec!(91.0), PivotKind::Low),
        pivot(4, dec!(120.5), PivotKind::High),
    ];
    // H&S itself fails; double top on the last 3 pivots may still match,
    // so we just verify the kind is NOT head_and_shoulders.
    let det_out = det.detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime());
    if let Some(d) = det_out {
        assert!(!matches!(d.kind, PatternKind::Classical(ref s) if s.starts_with("head_and_shoulders")));
    }
}

// ---------------------------------------------------------------------------
// Triangles
// ---------------------------------------------------------------------------

#[test]
fn detect_ascending_triangle() {
    let det = ClassicalDetector::new(ClassicalConfig::defaults()).unwrap();
    // flat resistance ~100, rising support 80 -> 88 (apex still in future).
    // H-L-H-L order so trailing 3 pivots can't masquerade as a double top.
    let pivots = vec![
        pivot(0, dec!(100.0), PivotKind::High),
        pivot(1, dec!(80.0), PivotKind::Low),
        pivot(2, dec!(100.1), PivotKind::High),
        pivot(3, dec!(88.0), PivotKind::Low),
    ];
    let d = det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .expect("ascending triangle should be detected");
    assert!(matches!(
        d.kind,
        PatternKind::Classical(ref s) if s == "ascending_triangle_bull"
    ));
}

#[test]
fn detect_descending_triangle() {
    let det = ClassicalDetector::new(ClassicalConfig::defaults()).unwrap();
    // flat support ~80, falling resistance 100 -> 92 (apex still in future).
    // L-H-L-H order so trailing 3 pivots can't masquerade as a double bottom.
    let pivots = vec![
        pivot(0, dec!(80.0), PivotKind::Low),
        pivot(1, dec!(100.0), PivotKind::High),
        pivot(2, dec!(80.1), PivotKind::Low),
        pivot(3, dec!(92.0), PivotKind::High),
    ];
    let d = det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .expect("descending triangle should be detected");
    assert!(matches!(
        d.kind,
        PatternKind::Classical(ref s) if s == "descending_triangle_bear"
    ));
}

#[test]
fn detect_symmetrical_triangle() {
    let det = ClassicalDetector::new(ClassicalConfig::defaults()).unwrap();
    // converging: highs 110 -> 100, lows 80 -> 90
    let pivots = vec![
        pivot(0, dec!(110.0), PivotKind::High),
        pivot(1, dec!(80.0), PivotKind::Low),
        pivot(2, dec!(100.0), PivotKind::High),
        pivot(3, dec!(90.0), PivotKind::Low),
    ];
    let d = det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .expect("symmetrical triangle should be detected");
    if let PatternKind::Classical(name) = &d.kind {
        assert!(name.contains("triangle"));
    } else {
        panic!("expected classical pattern");
    }
}

// ---------------------------------------------------------------------------
// Score floor
// ---------------------------------------------------------------------------

#[test]
fn detect_skips_when_score_floor_too_high() {
    let mut cfg = ClassicalConfig::defaults();
    cfg.min_structural_score = 0.99;
    let det = ClassicalDetector::new(cfg).unwrap();
    let pivots = vec![
        pivot(0, dec!(100.0), PivotKind::High),
        pivot(1, dec!(90.0), PivotKind::Low),
        pivot(2, dec!(101.5), PivotKind::High),
    ];
    assert!(det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .is_none());
}
