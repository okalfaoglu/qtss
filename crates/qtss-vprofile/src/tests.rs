use crate::builder::{BarBasedBuilder, VolumeProfileBuilder};
use crate::config::VProfileConfig;
use crate::naked::detect_naked_vpocs;
use chrono::{TimeZone, Utc};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
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

fn bar(idx: i64, open: Decimal, high: Decimal, low: Decimal, close: Decimal, vol: Decimal) -> Bar {
    Bar {
        instrument: instrument(),
        timeframe: Timeframe::H1,
        open_time: Utc.timestamp_opt(1_700_000_000 + idx * 3600, 0).unwrap(),
        open, high, low, close, volume: vol,
        closed: true,
    }
}

#[test]
fn config_defaults_validate() {
    VProfileConfig::defaults().validate().unwrap();
}

#[test]
fn bar_builder_rejects_thin_input() {
    let cfg = VProfileConfig::defaults();
    let bars: Vec<Bar> = (0..5).map(|i| bar(i, dec!(100), dec!(101), dec!(99), dec!(100), dec!(1))).collect();
    assert!(BarBasedBuilder.build(&bars, &cfg).is_err());
}

#[test]
fn bar_builder_emits_vpoc_at_dense_level() {
    let mut cfg = VProfileConfig::defaults();
    cfg.bin_count = 20;
    cfg.min_bars_for_profile = 10;
    // 30 bars: 25 trade tightly around 100, 5 spike to 105 — VPOC must
    // sit near 100.
    let mut bars = Vec::new();
    for i in 0..25 {
        bars.push(bar(i, dec!(99.5), dec!(100.5), dec!(99.5), dec!(100), dec!(10)));
    }
    for i in 25..30 {
        bars.push(bar(i, dec!(104), dec!(106), dec!(104), dec!(105), dec!(2)));
    }
    let prof = BarBasedBuilder.build(&bars, &cfg).unwrap();
    assert!(prof.vpoc > dec!(99) && prof.vpoc < dec!(102), "vpoc = {}", prof.vpoc);
    assert!(prof.val < prof.vpoc && prof.vah > prof.vpoc);
    assert!(prof.total_volume > dec!(0));
}

#[test]
fn naked_vpoc_marked_when_not_revisited() {
    let mut cfg = VProfileConfig::defaults();
    cfg.bin_count = 10;
    cfg.min_bars_for_profile = 5;
    let bars1: Vec<Bar> = (0..10).map(|i| bar(i, dec!(100), dec!(101), dec!(99), dec!(100), dec!(5))).collect();
    let prof1 = BarBasedBuilder.build(&bars1, &cfg).unwrap();
    // Subsequent bars trade in a different zone, never crossing prof1.vpoc.
    let after: Vec<Bar> = (10..20).map(|i| bar(i, dec!(120), dec!(121), dec!(119), dec!(120), dec!(5))).collect();
    let list = detect_naked_vpocs(&[(prof1.clone(), bars1[0].open_time, 9)], &after);
    assert!(list.levels[0].is_naked);
    let n = list.nearest_naked(dec!(122), false).unwrap();
    assert_eq!(n, prof1.vpoc);
}
