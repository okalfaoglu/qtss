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
    // 2 highs + 2 lows — the degenerate case. body_top drops the higher
    // of the two highs (potential UT → 101) and keeps 100 as resistance;
    // body_bottom drops the lower of the two lows (potential Spring →
    // 80) and keeps 81 as support.
    let pivots = vec![
        pivot(0, dec!(100), PivotKind::High, dec!(1)),
        pivot(1, dec!(80), PivotKind::Low, dec!(1)),
        pivot(2, dec!(101), PivotKind::High, dec!(1)),
        pivot(3, dec!(81), PivotKind::Low, dec!(1)),
    ];
    let r = TradingRange::from_pivots(&pivots).unwrap();
    assert!((r.resistance - 100.0).abs() < 0.01, "resistance={}", r.resistance);
    assert!((r.support - 81.0).abs() < 0.01, "support={}", r.support);
    assert!(r.height > 18.0);
}

#[test]
fn range_excludes_spring_and_ut_spikes() {
    // Wyckoff rule: Spring (70) and UT (120) pierce the range body.
    // Body highs [100, 101, 102] → resistance = mean(100,101) = 100.5
    // (UT 120 dropped). Body lows [80, 82, 81] → support = mean(81,82)
    // = 81.5 (Spring 70 dropped).
    let pivots = vec![
        pivot(0, dec!(100), PivotKind::High, dec!(1)),
        pivot(1, dec!(80),  PivotKind::Low,  dec!(1)),
        pivot(2, dec!(101), PivotKind::High, dec!(1)),
        pivot(3, dec!(82),  PivotKind::Low,  dec!(1)),
        pivot(4, dec!(102), PivotKind::High, dec!(1)),
        pivot(5, dec!(70),  PivotKind::Low,  dec!(1)), // Spring spike
        pivot(6, dec!(120), PivotKind::High, dec!(1)), // UT spike
        pivot(7, dec!(81),  PivotKind::Low,  dec!(1)),
    ];
    let r = TradingRange::from_pivots(&pivots).unwrap();
    // Body highs sorted [100, 101, 102, 120] — drop 120 (UT) → mean(100,101,102) = 101.
    assert!((r.resistance - 101.0).abs() < 0.01, "resistance={}", r.resistance);
    // Body lows sorted [70, 80, 81, 82] — drop 70 (Spring) → mean(80,81,82) = 81.
    assert!((r.support - 81.0).abs() < 0.01, "support={}", r.support);
    // Spring spike (70) must stay BELOW support; UT spike (120) ABOVE.
    assert!(70.0 < r.support);
    assert!(120.0 > r.resistance);
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
    // After the Wyckoff body-definition fix (Spring/UT pivots excluded
    // from the range body), the SC pivot at 80 sits BELOW the body
    // support — so the detector may surface the Automatic Rally /
    // Automatic Reaction event alongside the trading range. Either is
    // a legitimate Phase-A signature; we just require accumulation.
    match &d.kind {
        PatternKind::Wyckoff(name) => assert!(
            name.ends_with("_accumulation"),
            "expected accumulation Wyckoff event, got {name}"
        ),
        other => panic!("expected Wyckoff kind, got {other:?}"),
    }
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
    match &d.kind {
        PatternKind::Wyckoff(name) => assert!(
            name.ends_with("_distribution"),
            "expected distribution Wyckoff event, got {name}"
        ),
        other => panic!("expected Wyckoff kind, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Spring
// ---------------------------------------------------------------------------

#[test]
fn detect_spring() {
    let det = WyckoffDetector::new(WyckoffConfig::defaults()).unwrap();
    // A real Wyckoff Spring needs an ESTABLISHED range: multiple prior
    // support tests and meaningful time span. Fixture: a 30-bar trading
    // range with 3 lows at ~81 (repeated support tests), then a final
    // low at 78 poking ~15% below support → Spring.
    let pivots = vec![
        pivot(0,  dec!(100), PivotKind::High, dec!(1)),
        pivot(5,  dec!(81),  PivotKind::Low,  dec!(1)), // test #1
        pivot(10, dec!(99),  PivotKind::High, dec!(1)),
        pivot(15, dec!(81),  PivotKind::Low,  dec!(1)), // test #2
        pivot(20, dec!(100), PivotKind::High, dec!(1)),
        pivot(25, dec!(81),  PivotKind::Low,  dec!(1)), // test #3
        pivot(28, dec!(99),  PivotKind::High, dec!(1)),
        pivot(30, dec!(78),  PivotKind::Low,  dec!(1)), // spring
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
    // Mirror of Spring fixture: established range with 3 highs at ~99
    // (resistance tests), then an upthrust at 102 pokes above.
    let pivots = vec![
        pivot(0,  dec!(80),  PivotKind::Low,  dec!(1)),
        pivot(5,  dec!(99),  PivotKind::High, dec!(1)), // test #1
        pivot(10, dec!(81),  PivotKind::Low,  dec!(1)),
        pivot(15, dec!(99),  PivotKind::High, dec!(1)), // test #2
        pivot(20, dec!(80),  PivotKind::Low,  dec!(1)),
        pivot(25, dec!(99),  PivotKind::High, dec!(1)), // test #3
        pivot(28, dec!(81),  PivotKind::Low,  dec!(1)),
        pivot(30, dec!(102), PivotKind::High, dec!(1)), // upthrust
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
