use crate::config::ElliottConfig;
use crate::detector::ImpulseDetector;
use crate::fibs::{proximity_score, WAVE2_REFS, WAVE3_REFS};
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
