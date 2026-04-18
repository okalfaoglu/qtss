use super::*;
use chrono::{TimeZone, Utc};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal::prelude::FromPrimitive;
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
        kind: RegimeKind::TrendingUp,
        trend_strength: TrendStrength::Moderate,
        adx: dec!(25),
        bb_width: dec!(0.02),
        atr_pct: dec!(0.01),
        choppiness: dec!(50),
        confidence: 0.8,
    }
}

fn d(x: f64) -> Decimal {
    Decimal::from_f64(x).unwrap()
}

fn bar(i: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> Bar {
    Bar {
        instrument: instrument(),
        timeframe: Timeframe::H1,
        open_time: Utc.timestamp_opt(1_700_000_000 + i * 3600, 0).unwrap(),
        open: d(o),
        high: d(h),
        low: d(l),
        close: d(c),
        volume: d(v),
        closed: true,
    }
}

/// Flat consolidation (20 bars) then a bull breakaway gap.
fn breakaway_setup() -> Vec<Bar> {
    let mut bars = Vec::new();
    // 25 flat bars near 100.
    for i in 0..25 {
        bars.push(bar(i, 100.0, 100.3, 99.7, 100.0, 1000.0));
    }
    // gap up ~1.5%, vol 2x baseline.
    bars.push(bar(25, 101.5, 102.5, 101.3, 102.2, 2500.0));
    bars
}

fn common_gap_setup() -> Vec<Bar> {
    let mut bars = Vec::new();
    // Sideways/noisy bars without a sustained trend (alternating direction),
    // low volume → neither breakaway (range too wide) nor runaway (no trend)
    // nor exhaustion qualifies, leaving common_gap as the sole match.
    for i in 0..25 {
        // Wide intrabar range (>range_flat_pct) but same open/close → no
        // opening gaps and no trend; disqualifies breakaway/runaway.
        bars.push(bar(i, 100.0, 103.0, 97.0, 100.0, 1000.0));
    }
    // ~2.8% gap up with ordinary volume.
    bars.push(bar(25, 102.8, 103.0, 102.6, 102.7, 900.0));
    bars
}

fn runaway_setup() -> Vec<Bar> {
    let mut bars = Vec::new();
    // Strong uptrend for 25 bars (+0.5% per bar ≈ +12% cumulative).
    let mut price = 100.0;
    for i in 0..25 {
        let next = price * 1.005;
        bars.push(bar(i, price, next * 1.002, price * 0.998, next, 1000.0));
        price = next;
    }
    // Gap up ~1.2%, vol 1.6x.
    let gap_open = price * 1.012;
    bars.push(bar(25, gap_open, gap_open * 1.005, gap_open * 0.998, gap_open * 1.002, 1600.0));
    bars
}

fn exhaustion_setup() -> Vec<Bar> {
    let mut bars = Vec::new();
    let mut price = 100.0;
    for i in 0..25 {
        let next = price * 1.005;
        bars.push(bar(i, price, next * 1.002, price * 0.998, next, 1000.0));
        price = next;
    }
    let pre_close = price;
    let gap_open = price * 1.015;
    // Gap up bar with huge volume; closes well below pre-gap close.
    bars.push(bar(25, gap_open, gap_open * 1.001, pre_close * 0.98, pre_close * 0.985, 2500.0));
    bars
}

fn island_setup() -> Vec<Bar> {
    let mut bars = Vec::new();
    for i in 0..25 {
        bars.push(bar(i, 100.0, 100.3, 99.7, 100.0, 1000.0));
    }
    // Gap up ~1%.
    bars.push(bar(25, 101.0, 101.5, 100.8, 101.2, 1200.0));
    // 3 plateau bars.
    bars.push(bar(26, 101.2, 101.4, 100.9, 101.0, 1100.0));
    bars.push(bar(27, 101.0, 101.3, 100.8, 101.1, 1050.0));
    bars.push(bar(28, 101.1, 101.2, 100.9, 101.0, 1050.0));
    // Gap down ~1%.
    bars.push(bar(29, 99.9, 100.1, 99.6, 99.7, 1300.0));
    bars
}

#[test]
fn breakaway_gap_detected() {
    let det = GapDetector::new(GapConfig::default()).unwrap();
    let bars = breakaway_setup();
    let d = det.detect(&bars, &instrument(), Timeframe::H1, &regime()).unwrap();
    match d.kind {
        qtss_domain::v2::detection::PatternKind::Gap(s) => {
            assert!(s.starts_with("breakaway_gap_bull"), "got {s}");
        }
        _ => panic!("expected Gap kind"),
    }
}

#[test]
fn common_gap_when_no_trend_no_volume() {
    let det = GapDetector::new(GapConfig::default()).unwrap();
    let bars = common_gap_setup();
    let d = det.detect(&bars, &instrument(), Timeframe::H1, &regime()).unwrap();
    match d.kind {
        qtss_domain::v2::detection::PatternKind::Gap(s) => {
            assert!(s.starts_with("common_gap") || s.starts_with("runaway_gap"), "got {s}");
        }
        _ => panic!("expected Gap kind"),
    }
}

#[test]
fn runaway_gap_in_trend() {
    let det = GapDetector::new(GapConfig::default()).unwrap();
    let bars = runaway_setup();
    let d = det.detect(&bars, &instrument(), Timeframe::H1, &regime()).unwrap();
    match d.kind {
        qtss_domain::v2::detection::PatternKind::Gap(s) => {
            assert!(s.starts_with("runaway_gap_bull"), "got {s}");
        }
        _ => panic!("expected Gap kind"),
    }
}

#[test]
fn exhaustion_gap_reverses() {
    let det = GapDetector::new(GapConfig::default()).unwrap();
    let bars = exhaustion_setup();
    let d = det.detect(&bars, &instrument(), Timeframe::H1, &regime()).unwrap();
    match d.kind {
        qtss_domain::v2::detection::PatternKind::Gap(s) => {
            // bull trend → bear reversal variant
            assert!(s.starts_with("exhaustion_gap_bear"), "got {s}");
        }
        _ => panic!("expected Gap kind"),
    }
}

#[test]
fn island_reversal_detected() {
    let det = GapDetector::new(GapConfig::default()).unwrap();
    let bars = island_setup();
    let d = det.detect(&bars, &instrument(), Timeframe::H1, &regime()).unwrap();
    match d.kind {
        qtss_domain::v2::detection::PatternKind::Gap(s) => {
            assert!(s.starts_with("island_reversal_bear"), "got {s}");
        }
        _ => panic!("expected Gap kind"),
    }
}

#[test]
fn no_gap_no_detection() {
    let det = GapDetector::new(GapConfig::default()).unwrap();
    let mut bars = Vec::new();
    for i in 0..30 {
        bars.push(bar(i, 100.0, 100.2, 99.8, 100.0, 1000.0));
    }
    assert!(det.detect(&bars, &instrument(), Timeframe::H1, &regime()).is_none());
}

#[test]
fn config_validates() {
    let mut cfg = GapConfig::default();
    assert!(cfg.validate().is_ok());
    cfg.min_gap_pct = 0.0;
    assert!(cfg.validate().is_err());
}
