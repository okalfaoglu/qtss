use crate::config::HarmonicConfig;
use crate::detector::HarmonicDetector;
use crate::matcher::{match_pattern, RatioRange, XabcdPoints};
use crate::patterns::PATTERNS;
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

/// Build a textbook bullish Gartley:
///   XA = 100  (X=0,  A=100)
///   AB = 0.618 of XA (B = A - 61.8 = 38.2)
///   BC = 0.5 of AB  (C = B + 30.9 = 69.1)
///   CD = 1.272 of BC (D = C - 39.3 = 29.8)
///   AD ≈ 0.702 of XA  (close enough to 0.786 once we tweak D upward)
/// We just hand-pick numbers so each ratio falls inside the Gartley spec.
fn textbook_gartley_bull() -> Vec<Pivot> {
    vec![
        pivot(0, dec!(0),    PivotKind::Low),    // X
        pivot(1, dec!(100),  PivotKind::High),   // A
        pivot(2, dec!(38.2), PivotKind::Low),    // B  -> AB/XA = 0.618
        pivot(3, dec!(76.4), PivotKind::High),   // C  -> BC/AB = 0.618
        pivot(4, dec!(21.4), PivotKind::Low),    // D  -> CD/BC = 1.435, AD/XA = 0.786
    ]
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[test]
fn config_defaults_validate() {
    HarmonicConfig::defaults().validate().unwrap();
}

#[test]
fn config_rejects_excessive_slack() {
    let mut c = HarmonicConfig::defaults();
    c.global_slack = 0.9;
    assert!(c.validate().is_err());
}

// ---------------------------------------------------------------------------
// RatioRange
// ---------------------------------------------------------------------------

#[test]
fn ratio_range_contains_inside() {
    let r = RatioRange::new(0.5, 0.7);
    assert!(r.contains(0.6, 0.0));
    assert!(r.contains(0.5, 0.0));
    assert!(r.contains(0.7, 0.0));
}

#[test]
fn ratio_range_excludes_far_outside() {
    let r = RatioRange::new(0.5, 0.7);
    assert!(!r.contains(0.3, 0.0));
    assert!(!r.contains(0.9, 0.0));
}

#[test]
fn ratio_range_slack_widens_bounds() {
    let r = RatioRange::new(0.5, 0.7);
    assert!(!r.contains(0.46, 0.0));
    assert!(r.contains(0.46, 0.10));
}

// ---------------------------------------------------------------------------
// Catalog sanity
// ---------------------------------------------------------------------------

#[test]
fn pattern_catalog_has_expected_entries() {
    let names: Vec<&str> = PATTERNS.iter().map(|p| p.name).collect();
    assert!(names.contains(&"gartley"));
    assert!(names.contains(&"bat"));
    assert!(names.contains(&"butterfly"));
    assert!(names.contains(&"crab"));
    assert!(names.contains(&"deep_crab"));
    assert!(names.contains(&"shark"));
    assert!(names.contains(&"cypher"));
    assert!(names.contains(&"alt_bat"));
    assert!(names.contains(&"five_zero"));
    assert!(names.contains(&"three_drives"));
    assert!(names.contains(&"ab_cd"));
    assert!(names.contains(&"alt_ab_cd"));
}

// ---------------------------------------------------------------------------
// Matcher unit tests
// ---------------------------------------------------------------------------

#[test]
fn match_gartley_passes_on_canonical_ratios() {
    let pts = XabcdPoints {
        x: 0.0,
        a: 100.0,
        b: 38.2,
        c: 76.4,
        d: 21.4,
    };
    let gartley = PATTERNS.iter().find(|p| p.name == "gartley").unwrap();
    let s = match_pattern(gartley, &pts, 0.0).expect("gartley should match");
    assert!(s > 0.5, "score too low: {s}");
}

/// Textbook 5-0 (Scott Carney):
///   XA = 100 upmove: X=0, A=100
///   AB  = 1.3 × XA  ⇒ B = A − 130 = −30  (B extends below X — "not M/W")
///   BC  = 2.0 × AB  ⇒ C = B + 260 = 230   (BC = 2.0, mid of [1.618, 2.24])
///   CD  = 0.5 × BC  ⇒ D = C − 130 = 100   (D at 50% of BC)
/// Ratios: r_ab=1.30, r_bc=2.00, r_cd=0.50, r_ad=(A−D)/XA=0 — all in-spec.
/// (Note: at β=2.0 the Reciprocal AB=CD confluence hits exactly —
///  CD = 130 = AB, matching Carney's dual PRZ rule.)
#[test]
fn match_five_zero_passes_on_canonical_ratios() {
    let pts = XabcdPoints {
        x: 0.0,
        a: 100.0,
        b: -30.0,
        c: 230.0,
        d: 100.0,
    };
    let spec = PATTERNS.iter().find(|p| p.name == "five_zero").unwrap();
    let s = match_pattern(spec, &pts, 0.0).expect("5-0 should match canonical ratios");
    assert!(s > 0.5, "5-0 score too low: {s}");
    assert!(spec.extension, "5-0 must use D-anchored invalidation");
}

/// Regression guard: the *old* AD range `[0.84, 1.20]` would never
/// accept any canonical 5-0 because analytic r_ad ∈ [−0.20, +0.35].
/// Assert the current range covers those analytical extremes.
#[test]
fn five_zero_ad_range_covers_analytic_extremes() {
    let spec = PATTERNS.iter().find(|p| p.name == "five_zero").unwrap();
    // β=1.618, α=1.618 → r_ad ≈ +0.309 (largest positive)
    assert!(spec.ad.contains(0.309, 0.0));
    // β=2.24, α=1.618 → r_ad ≈ −0.194 (largest negative)
    assert!(spec.ad.contains(-0.194, 0.0));
    // β=2.00 → r_ad = 0 exactly
    assert!(spec.ad.contains(0.0, 0.0));
}

#[test]
fn match_returns_none_when_ratio_out_of_range() {
    let pts = XabcdPoints {
        x: 0.0,
        a: 100.0,
        b: 5.0,   // AB/XA = 0.95 (outside every spec for AB)
        c: 50.0,
        d: 20.0,
    };
    for spec in PATTERNS {
        assert!(match_pattern(spec, &pts, 0.0).is_none(), "{}", spec.name);
    }
}

#[test]
fn match_rejects_geometrically_invalid_legs() {
    // a < x  -> xa <= 0 -> ratios None.
    let pts = XabcdPoints {
        x: 100.0,
        a: 50.0,
        b: 30.0,
        c: 60.0,
        d: 10.0,
    };
    let gartley = PATTERNS.iter().find(|p| p.name == "gartley").unwrap();
    assert!(match_pattern(gartley, &pts, 0.0).is_none());
}

// ---------------------------------------------------------------------------
// Detector — end to end
// ---------------------------------------------------------------------------

#[test]
fn detect_returns_none_on_too_few_pivots() {
    let det = HarmonicDetector::new(HarmonicConfig::defaults()).unwrap();
    let tree = tree_from(vec![pivot(0, dec!(100), PivotKind::Low)]);
    assert!(det
        .detect(&tree, &instrument(), Timeframe::H4, &regime())
        .is_none());
}

#[test]
fn detect_finds_bullish_gartley() {
    let det = HarmonicDetector::new(HarmonicConfig::defaults()).unwrap();
    let tree = tree_from(textbook_gartley_bull());
    let d = det
        .detect(&tree, &instrument(), Timeframe::H4, &regime())
        .expect("textbook gartley should be detected");
    assert_eq!(d.kind, PatternKind::Harmonic("gartley_bull".into()));
    assert_eq!(d.anchors.len(), 5);
    assert_eq!(d.anchors[0].label.as_deref(), Some("X"));
    assert_eq!(d.anchors[4].label.as_deref(), Some("D"));
    assert_eq!(d.invalidation_price, dec!(0));
}

#[test]
fn detect_finds_bearish_mirror() {
    let bull = textbook_gartley_bull();
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
    let det = HarmonicDetector::new(HarmonicConfig::defaults()).unwrap();
    let d = det
        .detect(&tree_from(bear), &instrument(), Timeframe::H4, &regime())
        .expect("bearish mirror should also detect");
    assert_eq!(d.kind, PatternKind::Harmonic("gartley_bear".into()));
}

#[test]
fn detect_skips_when_score_floor_too_high() {
    let mut cfg = HarmonicConfig::defaults();
    cfg.min_structural_score = 0.99;
    let det = HarmonicDetector::new(cfg).unwrap();
    assert!(det
        .detect(
            &tree_from(textbook_gartley_bull()),
            &instrument(),
            Timeframe::H4,
            &regime()
        )
        .is_none());
}

#[test]
fn detect_uses_only_the_latest_five_pivots() {
    let mut pivots = vec![
        pivot(99, dec!(50), PivotKind::High),
        pivot(98, dec!(55), PivotKind::Low),
        pivot(97, dec!(40), PivotKind::High),
    ];
    pivots.extend(textbook_gartley_bull());
    let det = HarmonicDetector::new(HarmonicConfig::defaults()).unwrap();
    assert!(det
        .detect(
            &tree_from(pivots),
            &instrument(),
            Timeframe::H4,
            &regime()
        )
        .is_some());
}
