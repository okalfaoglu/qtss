use crate::combination::CombinationDetector;
use crate::config::ElliottConfig;
use crate::detector::ImpulseDetector;
use crate::fibs::{proximity_score, WAVE2_REFS, WAVE3_REFS};
use crate::formation::FormationDetector;
use crate::rules::{ImpulsePoints, RULES};
use chrono::{TimeZone, Utc};
use qtss_domain::v2::detection::PatternKind;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::pivot::{Pivot, PivotKind, PivotLevel, PivotTree};
use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

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
        kind: RegimeKind::TrendingUp,
        trend_strength: TrendStrength::Strong,
        adx: dec!(30),
        bb_width: dec!(0.05),
        atr_pct: dec!(0.02),
        choppiness: dec!(40),
        confidence: 0.8,
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

/// Construct a textbook bullish impulse with near-perfect Fibonacci ratios.
fn textbook_bullish_impulse() -> Vec<Pivot> {
    // Wave 1: 100 -> 110   (+10)
    // Wave 2: 110 -> 104   (retrace 0.6 of wave 1 = 6, near 0.618)
    // Wave 3: 104 -> 124   (+20, ext 2.0 of wave 1)
    // Wave 4: 124 -> 116   (retrace 0.4 of wave 3 = 8, near 0.382)
    // Wave 5: 116 -> 130   (+14)
    vec![
        pivot(0, dec!(100), PivotKind::Low),
        pivot(1, dec!(110), PivotKind::High),
        pivot(2, dec!(104), PivotKind::Low),
        pivot(3, dec!(124), PivotKind::High),
        pivot(4, dec!(116), PivotKind::Low),
        pivot(5, dec!(130), PivotKind::High),
    ]
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[test]
fn config_defaults_validate() {
    ElliottConfig::defaults().validate().unwrap();
}

#[test]
fn config_rejects_out_of_range_min_score() {
    let mut c = ElliottConfig::defaults();
    c.min_structural_score = 1.5;
    assert!(c.validate().is_err());
}

// ---------------------------------------------------------------------------
// Fib proximity
// ---------------------------------------------------------------------------

#[test]
fn proximity_perfect_hit_scores_one() {
    assert!((proximity_score(0.618, WAVE2_REFS) - 1.0).abs() < 1e-9);
}

#[test]
fn proximity_far_miss_scores_low() {
    let s = proximity_score(0.05, WAVE2_REFS);
    assert!(s < 0.05, "expected low score, got {s}");
}

#[test]
fn wave3_extension_scores_high_at_1618() {
    assert!(proximity_score(1.618, WAVE3_REFS) > 0.99);
}

// ---------------------------------------------------------------------------
// Rules — direct unit tests on ImpulsePoints
// ---------------------------------------------------------------------------

fn run_rules(p: ImpulsePoints) -> Result<(), &'static str> {
    let arr = p.as_f64();
    for r in RULES {
        r(&arr)?;
    }
    Ok(())
}

#[test]
fn rules_pass_on_textbook_impulse() {
    let p = ImpulsePoints {
        p0: dec!(100),
        p1: dec!(110),
        p2: dec!(104),
        p3: dec!(124),
        p4: dec!(116),
        p5: dec!(130),
    };
    run_rules(p).unwrap();
}

#[test]
fn rules_reject_wave2_break_below_start() {
    let p = ImpulsePoints {
        p0: dec!(100),
        p1: dec!(110),
        p2: dec!(99), // dropped below p0
        p3: dec!(124),
        p4: dec!(116),
        p5: dec!(130),
    };
    assert_eq!(run_rules(p), Err("wave 2 retraced past wave 1 start"));
}

#[test]
fn rules_reject_wave3_shortest() {
    // w1 = 20, w3 = 5, w5 = 25 -> w3 shortest
    let p = ImpulsePoints {
        p0: dec!(100),
        p1: dec!(120),
        p2: dec!(112),
        p3: dec!(117),
        p4: dec!(115),
        p5: dec!(140),
    };
    assert_eq!(run_rules(p), Err("wave 3 is the shortest"));
}

#[test]
fn rules_reject_wave4_overlap() {
    // w4 dips to 108, but w1 top was 110 -> overlap.
    let p = ImpulsePoints {
        p0: dec!(100),
        p1: dec!(110),
        p2: dec!(105),
        p3: dec!(125),
        p4: dec!(108), // back below w1 top -> overlap
        p5: dec!(135),
    };
    assert_eq!(run_rules(p), Err("wave 4 overlaps wave 1"));
}

// ---------------------------------------------------------------------------
// Detector — end to end on a synthetic tree
// ---------------------------------------------------------------------------

#[test]
fn detect_returns_none_on_too_few_pivots() {
    let det = ImpulseDetector::new(ElliottConfig::defaults()).unwrap();
    let tree = tree_from(vec![pivot(0, dec!(100), PivotKind::Low)]);
    assert!(det
        .detect(&tree, &instrument(), Timeframe::H4, &regime())
        .is_none());
}

#[test]
fn detect_finds_textbook_bullish_impulse() {
    let det = ImpulseDetector::new(ElliottConfig::defaults()).unwrap();
    let tree = tree_from(textbook_bullish_impulse());
    let d = det
        .detect(&tree, &instrument(), Timeframe::H4, &regime())
        .expect("textbook impulse should be detected");
    assert_eq!(
        d.kind,
        PatternKind::Elliott("impulse_5_bull".into())
    );
    assert_eq!(d.anchors.len(), 6);
    assert_eq!(d.anchors[0].label.as_deref(), Some("0"));
    assert_eq!(d.anchors[5].label.as_deref(), Some("5"));
    assert!(d.structural_score >= 0.40);
    assert_eq!(d.invalidation_price, dec!(100));
}

#[test]
fn detect_finds_bearish_mirror_impulse() {
    // Mirror image around 200: each price = 200 - bullish_price.
    let bull = textbook_bullish_impulse();
    let bear: Vec<Pivot> = bull
        .iter()
        .map(|p| Pivot {
            bar_index: p.bar_index,
            time: p.time,
            price: dec!(200) - p.price,
            kind: match p.kind {
                PivotKind::High => PivotKind::Low,
                PivotKind::Low => PivotKind::High,
            },
            level: p.level,
            prominence: p.prominence,
            volume_at_pivot: p.volume_at_pivot,
            swing_type: None,
        })
        .collect();
    let det = ImpulseDetector::new(ElliottConfig::defaults()).unwrap();
    let d = det
        .detect(
            &tree_from(bear),
            &instrument(),
            Timeframe::H4,
            &regime(),
        )
        .expect("bearish mirror should also detect");
    assert_eq!(
        d.kind,
        PatternKind::Elliott("impulse_5_bear".into())
    );
}

#[test]
fn detect_skips_when_score_below_floor() {
    // A geometrically valid 1-2-3-4-5 with poor Fib ratios should be
    // suppressed when the floor is high.
    let mut cfg = ElliottConfig::defaults();
    cfg.min_structural_score = 0.99;
    let det = ImpulseDetector::new(cfg).unwrap();
    let tree = tree_from(textbook_bullish_impulse());
    assert!(det
        .detect(&tree, &instrument(), Timeframe::H4, &regime())
        .is_none());
}

#[test]
fn detect_rejects_overlap_violation() {
    let bad = vec![
        pivot(0, dec!(100), PivotKind::Low),
        pivot(1, dec!(110), PivotKind::High),
        pivot(2, dec!(105), PivotKind::Low),
        pivot(3, dec!(125), PivotKind::High),
        pivot(4, dec!(108), PivotKind::Low), // overlaps wave 1 top (110)
        pivot(5, dec!(135), PivotKind::High),
    ];
    let det = ImpulseDetector::new(ElliottConfig::defaults()).unwrap();
    assert!(det
        .detect(&tree_from(bad), &instrument(), Timeframe::H4, &regime())
        .is_none());
}

#[test]
fn detect_uses_only_the_latest_six_pivots() {
    // Prepend noise; the detector must scan only the tail.
    let mut pivots = vec![
        pivot(99, dec!(50), PivotKind::High),
        pivot(98, dec!(55), PivotKind::Low),
    ];
    pivots.extend(textbook_bullish_impulse());
    let det = ImpulseDetector::new(ElliottConfig::defaults()).unwrap();
    assert!(det
        .detect(
            &tree_from(pivots),
            &instrument(),
            Timeframe::H4,
            &regime()
        )
        .is_some());
}

// ---------------------------------------------------------------------------
// Combination (W-X-Y) detector
// ---------------------------------------------------------------------------

/// Build a textbook bearish W-X-Y combination (correcting a prior bull leg).
/// W = downward zigzag: H100→L80→H90→L70  (A=-20, B=+10, C=-20)
/// X = retracement: L70→H85  (retraces 50% of W range 30)
/// Y = downward zigzag: H85→L65→H75→L55  (A=-20, B=+10, C=-20)
fn textbook_wxy_bear() -> Vec<Pivot> {
    vec![
        pivot(0,  dec!(100), PivotKind::High),  // W start
        pivot(1,  dec!(80),  PivotKind::Low),   // W-A
        pivot(2,  dec!(90),  PivotKind::High),  // W-B
        pivot(3,  dec!(70),  PivotKind::Low),   // W-C
        pivot(4,  dec!(85),  PivotKind::High),  // X end / Y start
        pivot(5,  dec!(65),  PivotKind::Low),   // Y-A
        pivot(6,  dec!(75),  PivotKind::High),  // Y-B
        pivot(7,  dec!(55),  PivotKind::Low),   // Y-C
    ]
}

/// Build a bullish W-X-Y (correcting a prior bear leg).
fn textbook_wxy_bull() -> Vec<Pivot> {
    vec![
        pivot(0,  dec!(50),  PivotKind::Low),    // W start
        pivot(1,  dec!(70),  PivotKind::High),   // W-A
        pivot(2,  dec!(60),  PivotKind::Low),    // W-B
        pivot(3,  dec!(80),  PivotKind::High),   // W-C
        pivot(4,  dec!(65),  PivotKind::Low),    // X end / Y start
        pivot(5,  dec!(85),  PivotKind::High),   // Y-A
        pivot(6,  dec!(75),  PivotKind::Low),    // Y-B
        pivot(7,  dec!(95),  PivotKind::High),   // Y-C
    ]
}

#[test]
fn combo_returns_none_on_too_few_pivots() {
    let det = CombinationDetector::new(ElliottConfig::defaults()).unwrap();
    let tree = tree_from(vec![
        pivot(0, dec!(100), PivotKind::High),
        pivot(1, dec!(90), PivotKind::Low),
        pivot(2, dec!(95), PivotKind::High),
    ]);
    assert!(det.detect(&tree, &instrument(), Timeframe::H4, &regime()).is_empty());
}

#[test]
fn combo_detects_bearish_wxy() {
    let det = CombinationDetector::new(ElliottConfig::defaults()).unwrap();
    let tree = tree_from(textbook_wxy_bear());
    let results = det.detect(&tree, &instrument(), Timeframe::H4, &regime());
    assert!(!results.is_empty(), "bearish W-X-Y should be detected");
    let d = &results[0];
    if let PatternKind::Elliott(ref name) = d.kind {
        assert!(name.contains("combination_wxy"), "got {name}");
        assert!(name.ends_with("_bear"), "got {name}");
    } else {
        panic!("expected Elliott pattern kind");
    }
    assert_eq!(d.anchors.len(), 8);
    assert_eq!(d.invalidation_price, dec!(100)); // W start
}

#[test]
fn combo_detects_bullish_wxy() {
    let det = CombinationDetector::new(ElliottConfig::defaults()).unwrap();
    let tree = tree_from(textbook_wxy_bull());
    let results = det.detect(&tree, &instrument(), Timeframe::H4, &regime());
    assert!(!results.is_empty(), "bullish W-X-Y should be detected");
    let d = &results[0];
    if let PatternKind::Elliott(ref name) = d.kind {
        assert!(name.contains("combination_wxy"), "got {name}");
        assert!(name.ends_with("_bull"), "got {name}");
    } else {
        panic!("expected Elliott pattern kind");
    }
}

#[test]
fn combo_rejects_when_x_too_deep() {
    // X retraces 100% of W — that's not a connecting wave.
    let pivots = vec![
        pivot(0, dec!(100), PivotKind::High),
        pivot(1, dec!(80), PivotKind::Low),
        pivot(2, dec!(90), PivotKind::High),
        pivot(3, dec!(70), PivotKind::Low),
        pivot(4, dec!(100), PivotKind::High),  // X = full retrace
        pivot(5, dec!(65), PivotKind::Low),
        pivot(6, dec!(75), PivotKind::High),
        pivot(7, dec!(55), PivotKind::Low),
    ];
    let det = CombinationDetector::new(ElliottConfig::defaults()).unwrap();
    assert!(det.detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime()).is_empty());
}

#[test]
fn combo_rejects_when_y_goes_opposite() {
    // Y goes up instead of continuing W's direction (down).
    let pivots = vec![
        pivot(0, dec!(100), PivotKind::High),
        pivot(1, dec!(80), PivotKind::Low),
        pivot(2, dec!(90), PivotKind::High),
        pivot(3, dec!(70), PivotKind::Low),    // W end
        pivot(4, dec!(85), PivotKind::High),   // X end
        pivot(5, dec!(95), PivotKind::Low),    // Y goes opposite
        pivot(6, dec!(105), PivotKind::High),  // Y-B
        pivot(7, dec!(110), PivotKind::Low),   // Y end — wrong direction
    ];
    let det = CombinationDetector::new(ElliottConfig::defaults()).unwrap();
    assert!(det.detect(&tree_from(pivots), &instrument(), Timeframe::H4, &regime()).is_empty());
}

#[test]
fn combo_skips_when_score_floor_too_high() {
    // Use a geometrically valid but ratio-imperfect W-X-Y so that the
    // structural score stays well below 0.99.
    let mut cfg = ElliottConfig::defaults();
    cfg.min_structural_score = 0.99;
    let det = CombinationDetector::new(cfg).unwrap();
    let imperfect = vec![
        pivot(0,  dec!(100), PivotKind::High),
        pivot(1,  dec!(82),  PivotKind::Low),    // W-A
        pivot(2,  dec!(93),  PivotKind::High),   // W-B (non-standard retrace)
        pivot(3,  dec!(71),  PivotKind::Low),    // W-C
        pivot(4,  dec!(84),  PivotKind::High),   // X end
        pivot(5,  dec!(68),  PivotKind::Low),    // Y-A
        pivot(6,  dec!(77),  PivotKind::High),   // Y-B
        pivot(7,  dec!(52),  PivotKind::Low),    // Y-C
    ];
    assert!(det
        .detect(&tree_from(imperfect), &instrument(), Timeframe::H4, &regime())
        .is_empty());
}

// ---------------------------------------------------------------------------
// LuxAlgo ZigZag + Motive + Corrective pipeline tests
// ---------------------------------------------------------------------------

#[test]
fn luxalgo_zigzag_pivot_detection() {
    use crate::zigzag::{ZigZag, process_bar};
    use qtss_domain::v2::bar::Bar;
    use chrono::Utc;

    let mut zz = ZigZag::new(11);
    let mut bars = Vec::new();

    // Build simple uptrend with alternating pivots.
    let base_time = Utc::now();
    for (i, (h, l, c)) in [
        (100.0, 95.0, 99.0),
        (105.0, 100.0, 104.0),
        (110.0, 105.0, 109.0), // Pivot high at 110
        (108.0, 103.0, 105.0),
        (115.0, 105.0, 114.0), // Next pivot high at 115
    ]
    .iter()
    .enumerate()
    {
        bars.push(Bar {
            instrument: instrument(),
            timeframe: Timeframe::M1,
            open_time: base_time + chrono::Duration::minutes(i as i64),
            open: dec!(*c),
            high: dec!(*h),
            low: dec!(*l),
            close: dec!(*c),
            volume: dec!(1000),
            closed: true,
        });
        process_bar(&mut zz, &bars, 4, 4);
    }

    // Should detect pivot highs
    assert!(zz.last().is_some());
}

#[test]
fn luxalgo_motive_wave_bullish() {
    use crate::motive::detect_motive;
    use crate::zigzag::ZigZagPoint;

    let points = vec![
        ZigZagPoint {
            bars_ago: 4,
            price: 100.0,
            direction: 1,
        },
        ZigZagPoint {
            bars_ago: 3,
            price: 95.0,
            direction: -1,
        },
        ZigZagPoint {
            bars_ago: 2,
            price: 110.0,
            direction: 1,
        },
        ZigZagPoint {
            bars_ago: 1,
            price: 105.0,
            direction: -1,
        },
        ZigZagPoint {
            bars_ago: 0,
            price: 115.0,
            direction: 1,
        },
    ];

    let motive = detect_motive(&points);
    assert!(motive.is_some());
    let m = motive.unwrap();
    assert_eq!(m.direction, 1);
    assert!(m.score > 0.0);
}

#[test]
fn luxalgo_corrective_wave_abc() {
    use crate::corrective::detect_corrective;
    use crate::zigzag::ZigZagPoint;

    let points = vec![
        ZigZagPoint {
            bars_ago: 2,
            price: 100.0,
            direction: 1,
        },
        ZigZagPoint {
            bars_ago: 1,
            price: 95.0,
            direction: -1,
        },
        ZigZagPoint {
            bars_ago: 0,
            price: 85.0,
            direction: 1,
        },
    ];

    let corr = detect_corrective(&points);
    assert!(corr.is_some());
    let c = corr.unwrap();
    assert_eq!(c.direction, -1); // Downward correction
    assert!(c.score > 0.0);
}

#[test]
fn luxalgo_detector_integration() {
    use crate::aggregator::{ElliottDetectorSet, ElliottFormationToggles};
    use crate::formation::FormationDetector;

    let mut toggles = ElliottFormationToggles::defaults();
    toggles.impulse = false; // Disable other detectors for cleaner test
    toggles.nascent_impulse = false;
    toggles.forming_impulse = false;
    toggles.leading_diagonal = false;
    toggles.ending_diagonal = false;
    toggles.zigzag = false;
    toggles.flat = false;
    toggles.triangle = false;
    toggles.extended_impulse = false;
    toggles.truncated_fifth = false;
    toggles.combination = false;

    let mut config = ElliottConfig::defaults();
    config.pivot_level = PivotLevel::L1;

    let detector_set = ElliottDetectorSet::new(config, &toggles).unwrap();

    // Build a simple 5-pivot impulse structure
    let pivots = vec![
        pivot(0, dec!(100), PivotKind::Low),
        pivot(1, dec!(110), PivotKind::High),
        pivot(2, dec!(105), PivotKind::Low),
        pivot(3, dec!(120), PivotKind::High),
        pivot(4, dec!(115), PivotKind::Low),
    ];

    let tree = tree_from(pivots);
    let detections = detector_set.detect_all(&tree, &instrument(), Timeframe::H4, &regime());

    // Should detect at least one pattern (motive or corrective)
    assert!(!detections.is_empty());
}
