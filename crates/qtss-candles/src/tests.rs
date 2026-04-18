use super::*;
use chrono::{TimeZone, Utc};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::detection::PatternKind;
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
        kind: RegimeKind::Ranging,
        trend_strength: TrendStrength::Moderate,
        adx: dec!(20),
        bb_width: dec!(0.02),
        atr_pct: dec!(0.01),
        choppiness: dec!(50),
        confidence: 0.8,
    }
}

fn d(x: f64) -> Decimal {
    Decimal::from_f64(x).unwrap()
}

fn bar(i: i64, o: f64, h: f64, l: f64, c: f64) -> Bar {
    Bar {
        instrument: instrument(),
        timeframe: Timeframe::H1,
        open_time: Utc.timestamp_opt(1_700_000_000 + i * 3600, 0).unwrap(),
        open: d(o),
        high: d(h),
        low: d(l),
        close: d(c),
        volume: d(1000.0),
        closed: true,
    }
}

/// Produces N bars of down-trending price ending at `end_close`.
fn downtrend(n: usize, start: f64, end_close: f64) -> Vec<Bar> {
    let step = (end_close - start) / n as f64;
    (0..n)
        .map(|i| {
            let o = start + step * i as f64;
            let c = start + step * (i + 1) as f64;
            let hi = o.max(c) + 0.1;
            let lo = o.min(c) - 0.1;
            bar(i as i64, o, hi, lo, c)
        })
        .collect()
}

fn uptrend(n: usize, start: f64, end_close: f64) -> Vec<Bar> {
    downtrend(n, start, end_close) // symmetric when end_close > start
}

#[test]
fn hammer_after_downtrend() {
    let det = CandleDetector::new(CandleConfig::default()).unwrap();
    // 10 bars of ~3% downtrend.
    let mut bars = downtrend(10, 100.0, 96.0);
    // Hammer: small body near top, long lower shadow.
    // Body ~0.5 (open 96.0 → close 96.5), lower shadow ~2.5, upper shadow ~0.1.
    bars.push(bar(10, 96.0, 96.6, 93.5, 96.5));
    let d = det.detect(&bars, &instrument(), Timeframe::H1, &regime()).unwrap();
    match d.kind {
        PatternKind::Candle(s) => assert!(
            s.starts_with("hammer_bull") || s.starts_with("engulfing_bull"),
            "got {s}"
        ),
        _ => panic!("expected Candle"),
    }
}

#[test]
fn shooting_star_after_uptrend() {
    let det = CandleDetector::new(CandleConfig::default()).unwrap();
    let mut bars = uptrend(10, 96.0, 100.0);
    // Shooting star: small body near bottom, long upper shadow, bearish-ish.
    // Body ~0.4, upper shadow ~2.1, lower shadow ~0.1.
    bars.push(bar(10, 100.0, 102.5, 99.9, 99.6));
    let d = det.detect(&bars, &instrument(), Timeframe::H1, &regime()).unwrap();
    match d.kind {
        PatternKind::Candle(s) => assert!(s.starts_with("shooting_star_bear"), "got {s}"),
        _ => panic!("expected Candle"),
    }
}

#[test]
fn bullish_engulfing() {
    let det = CandleDetector::new(CandleConfig::default()).unwrap();
    let mut bars = downtrend(10, 100.0, 96.0);
    bars.push(bar(10, 96.0, 96.1, 95.5, 95.6));   // prev bear small
    bars.push(bar(11, 95.5, 97.5, 95.4, 97.2));   // curr bull engulfing
    let d = det.detect(&bars, &instrument(), Timeframe::H1, &regime()).unwrap();
    match d.kind {
        PatternKind::Candle(s) => assert!(s.starts_with("engulfing_bull") || s.starts_with("three_outside_up"), "got {s}"),
        _ => panic!("expected Candle"),
    }
}

#[test]
fn morning_star() {
    let det = CandleDetector::new(CandleConfig::default()).unwrap();
    let mut bars = downtrend(10, 100.0, 96.0);
    bars.push(bar(10, 96.0, 96.1, 94.0, 94.2));   // big bear
    bars.push(bar(11, 94.0, 94.3, 93.8, 94.0));   // small body
    bars.push(bar(12, 94.1, 96.3, 94.0, 96.2));   // big bull closing past midpoint
    let d = det.detect(&bars, &instrument(), Timeframe::H1, &regime()).unwrap();
    match d.kind {
        PatternKind::Candle(s) => assert!(s.starts_with("morning_star_bull"), "got {s}"),
        _ => panic!("expected Candle"),
    }
}

#[test]
fn doji_flat_market() {
    let det = CandleDetector::new(CandleConfig::default()).unwrap();
    let mut bars = Vec::new();
    for i in 0..10 {
        bars.push(bar(i, 100.0, 100.3, 99.7, 100.0));
    }
    // Doji: open == close, long shadows on both sides.
    bars.push(bar(10, 100.0, 100.8, 99.2, 100.02));
    let d = det.detect(&bars, &instrument(), Timeframe::H1, &regime()).unwrap();
    match d.kind {
        PatternKind::Candle(s) => assert!(s.contains("doji"), "got {s}"),
        _ => panic!("expected Candle"),
    }
}

#[test]
fn no_pattern_plain_bars() {
    let det = CandleDetector::new(CandleConfig::default()).unwrap();
    // Plain bars with ~45% body and even shadows — no clear pattern.
    let mut bars = Vec::new();
    for i in 0..15 {
        bars.push(bar(i, 100.0, 100.3, 99.7, 100.15));
    }
    assert!(det.detect(&bars, &instrument(), Timeframe::H1, &regime()).is_none());
}

#[test]
fn config_validates() {
    let cfg = CandleConfig::default();
    assert!(cfg.validate().is_ok());
    let bad = CandleConfig {
        trend_context_bars: 1,
        ..CandleConfig::default()
    };
    assert!(bad.validate().is_err());
}
