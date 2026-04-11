use crate::config::WyckoffConfig;
use crate::detector::WyckoffDetector;
use crate::events::EVENTS;
use crate::range::TradingRange;
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
        adx: dec!(15),
        bb_width: dec!(0.05),
        atr_pct: dec!(0.02),
        choppiness: dec!(70),
        confidence: 0.7,
    }
}

fn pivot(idx: u64, price: Decimal, kind: PivotKind, vol: Decimal) -> Pivot {
    Pivot {
        bar_index: idx,
        time: Utc.timestamp_opt(1_700_000_000 + idx as i64 * 60, 0).unwrap(),
        price,
        kind,
        level: PivotLevel::L1,
        prominence: dec!(1),
        volume_at_pivot: vol,
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
    WyckoffConfig::defaults().validate().unwrap();
}

#[test]
fn config_rejects_bad_min_pivots() {
    let mut c = WyckoffConfig::defaults();
    c.min_range_pivots = 3;
    assert!(c.validate().is_err());
}

#[test]
fn config_rejects_low_climax_mult() {
    let mut c = WyckoffConfig::defaults();
    c.climax_volume_mult = 0.9;
    assert!(c.validate().is_err());
}

#[test]
fn config_rejects_swapped_penetration() {
    let mut c = WyckoffConfig::defaults();
    c.min_penetration = 0.4;
    c.max_penetration = 0.2;
    assert!(c.validate().is_err());
}

// ---------------------------------------------------------------------------
// TradingRange helper
// ---------------------------------------------------------------------------

#[test]
fn range_from_box_pivots() {
    let pivots = vec![
        pivot(0, dec!(100), PivotKind::High, dec!(1)),
        pivot(1, dec!(80), PivotKind::Low, dec!(1)),
        pivot(2, dec!(101), PivotKind::High, dec!(1)),
        pivot(3, dec!(81), PivotKind::Low, dec!(1)),
    ];
    let r = TradingRange::from_pivots(&pivots).unwrap();
    assert!((r.resistance - 100.5).abs() < 0.01);
    assert!((r.support - 80.5).abs() < 0.01);
    assert!(r.height > 19.0);
}

#[test]
fn range_rejects_when_no_alternation() {
    let pivots = vec![
        pivot(0, dec!(100), PivotKind::High, dec!(1)),
        pivot(1, dec!(101), PivotKind::High, dec!(1)),
        pivot(2, dec!(102), PivotKind::High, dec!(1)),
    ];
    assert!(TradingRange::from_pivots(&pivots).is_none());
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

#[test]
fn catalog_has_expected_events() {
    let names: Vec<&str> = EVENTS.iter().map(|e| e.name).collect();
    for n in ["trading_range", "spring", "upthrust"] {
        assert!(names.contains(&n), "missing {n}");
    }
}

// ---------------------------------------------------------------------------
// Detector — too few pivots
// ---------------------------------------------------------------------------

#[test]
fn detect_returns_none_on_too_few_pivots() {
    let det = WyckoffDetector::new(WyckoffConfig::defaults()).unwrap();
    let tree = tree_from(vec![pivot(0, dec!(100), PivotKind::High, dec!(1))]);
    assert!(det
        .detect(&tree, &instrument(), Timeframe::H4, &regime())
        .is_none());
}

// ---------------------------------------------------------------------------
// Trading range
// ---------------------------------------------------------------------------

#[test]
fn detect_accumulation_range() {
    // 5 pivots oscillating in [80,100], a low has climactic volume.
    let det = WyckoffDetector::new(WyckoffConfig::defaults()).unwrap();
    let pivots = vec![
        pivot(0, dec!(100), PivotKind::High, dec!(1)),
        pivot(1, dec!(80), PivotKind::Low, dec!(5)), // Selling Climax
        pivot(2, dec!(99), PivotKind::High, dec!(1)),
        pivot(3, dec!(81), PivotKind::Low, dec!(1)),
        pivot(4, dec!(100), PivotKind::High, dec!(1)),
    ];
    let d = det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .expect("trading range should be detected");
    assert_eq!(
        d.kind,
        PatternKind::Wyckoff("trading_range_accumulation".into())
    );
}

#[test]
fn detect_distribution_range() {
    let det = WyckoffDetector::new(WyckoffConfig::defaults()).unwrap();
    let pivots = vec![
        pivot(0, dec!(80), PivotKind::Low, dec!(1)),
        pivot(1, dec!(100), PivotKind::High, dec!(5)), // Buying Climax
        pivot(2, dec!(81), PivotKind::Low, dec!(1)),
        pivot(3, dec!(99), PivotKind::High, dec!(1)),
        pivot(4, dec!(80), PivotKind::Low, dec!(1)),
    ];
    let d = det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .expect("distribution range should be detected");
    assert_eq!(
        d.kind,
        PatternKind::Wyckoff("trading_range_distribution".into())
    );
}

// ---------------------------------------------------------------------------
// Spring
// ---------------------------------------------------------------------------

#[test]
fn detect_spring() {
    let det = WyckoffDetector::new(WyckoffConfig::defaults()).unwrap();
    // 5-pivot range plus a final low that pokes ~10% below support.
    // Range height ~20 → penetration 2/20 = 0.10, in band [0.02, 0.30].
    let pivots = vec![
        pivot(0, dec!(100), PivotKind::High, dec!(1)),
        pivot(1, dec!(80), PivotKind::Low, dec!(1)),
        pivot(2, dec!(99), PivotKind::High, dec!(1)),
        pivot(3, dec!(81), PivotKind::Low, dec!(1)),
        pivot(4, dec!(100), PivotKind::High, dec!(1)),
        pivot(5, dec!(78), PivotKind::Low, dec!(1)), // spring
    ];
    let d = det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .expect("spring should be detected");
    assert_eq!(d.kind, PatternKind::Wyckoff("spring_bull".into()));
    assert_eq!(d.invalidation_price, dec!(78));
}

#[test]
fn detect_spring_rejected_when_too_deep() {
    // Same range, but the penetration is huge (price collapses 50% below
    // support) — that's a real breakdown, not a spring.
    let det = WyckoffDetector::new(WyckoffConfig::defaults()).unwrap();
    let pivots = vec![
        pivot(0, dec!(100), PivotKind::High, dec!(1)),
        pivot(1, dec!(80), PivotKind::Low, dec!(1)),
        pivot(2, dec!(99), PivotKind::High, dec!(1)),
        pivot(3, dec!(81), PivotKind::Low, dec!(1)),
        pivot(4, dec!(100), PivotKind::High, dec!(1)),
        pivot(5, dec!(40), PivotKind::Low, dec!(1)), // 50%+ penetration
    ];
    let det_out = det.detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime());
    if let Some(d) = det_out {
        assert!(!matches!(d.kind, PatternKind::Wyckoff(ref s) if s.starts_with("spring")));
    }
}

// ---------------------------------------------------------------------------
// Upthrust
// ---------------------------------------------------------------------------

#[test]
fn detect_upthrust() {
    let det = WyckoffDetector::new(WyckoffConfig::defaults()).unwrap();
    let pivots = vec![
        pivot(0, dec!(80), PivotKind::Low, dec!(1)),
        pivot(1, dec!(100), PivotKind::High, dec!(1)),
        pivot(2, dec!(81), PivotKind::Low, dec!(1)),
        pivot(3, dec!(99), PivotKind::High, dec!(1)),
        pivot(4, dec!(80), PivotKind::Low, dec!(1)),
        pivot(5, dec!(102), PivotKind::High, dec!(1)), // upthrust ~7.5% over
    ];
    let d = det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .expect("upthrust should be detected");
    assert_eq!(d.kind, PatternKind::Wyckoff("upthrust_bear".into()));
    assert_eq!(d.invalidation_price, dec!(102));
}

// ---------------------------------------------------------------------------
// Score floor
// ---------------------------------------------------------------------------

#[test]
fn detect_skips_when_score_floor_too_high() {
    let mut cfg = WyckoffConfig::defaults();
    cfg.min_structural_score = 0.99;
    let det = WyckoffDetector::new(cfg).unwrap();
    let pivots = vec![
        pivot(0, dec!(100), PivotKind::High, dec!(1)),
        pivot(1, dec!(80), PivotKind::Low, dec!(1)),
        pivot(2, dec!(99), PivotKind::High, dec!(1)),
        pivot(3, dec!(81), PivotKind::Low, dec!(1)),
        pivot(4, dec!(100), PivotKind::High, dec!(1)),
    ];
    assert!(det
        .detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime())
        .is_none());
}
