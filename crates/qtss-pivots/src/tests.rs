use crate::atr::AtrState;
use crate::config::PivotConfig;
use crate::engine::PivotEngine;
use crate::error::PivotError;
use chrono::{DateTime, Duration, TimeZone, Utc};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::pivot::{PivotKind, PivotLevel};
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

fn t(i: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + i * 60, 0).unwrap()
}

fn bar(i: i64, o: Decimal, h: Decimal, l: Decimal, c: Decimal) -> Bar {
    Bar {
        instrument: instrument(),
        timeframe: Timeframe::M1,
        open_time: t(i),
        open: o,
        high: h,
        low: l,
        close: c,
        volume: dec!(1),
        closed: true,
    }
}

/// Symmetric bar: triangular (price = open = close = mid).
fn flat(i: i64, p: Decimal) -> Bar {
    bar(i, p, p, p, p)
}

// ---------------------------------------------------------------------------
// PivotConfig validation
// ---------------------------------------------------------------------------

#[test]
fn config_defaults_validate() {
    PivotConfig::defaults().validate().unwrap();
}

#[test]
fn config_rejects_short_atr_period() {
    let mut c = PivotConfig::defaults();
    c.atr_period = 1;
    assert!(matches!(c.validate(), Err(PivotError::InvalidConfig(_))));
}

#[test]
fn config_rejects_non_increasing_multipliers() {
    let mut c = PivotConfig::defaults();
    c.atr_mult = [dec!(1), dec!(2), dec!(2), dec!(3)];
    assert!(matches!(c.validate(), Err(PivotError::InvalidConfig(_))));
}

// ---------------------------------------------------------------------------
// ATR
// ---------------------------------------------------------------------------

#[test]
fn atr_warms_up_then_smooths() {
    let mut atr = AtrState::new(3);
    assert_eq!(atr.update(dec!(10), dec!(8), dec!(9)), None);
    assert_eq!(atr.update(dec!(11), dec!(9), dec!(10)), None);
    let v = atr.update(dec!(12), dec!(10), dec!(11)).unwrap();
    assert!(v > dec!(0), "ATR should be positive after warm-up");
    let next = atr.update(dec!(15), dec!(11), dec!(14)).unwrap();
    assert!(next > dec!(0));
}

// ---------------------------------------------------------------------------
// PivotEngine — end-to-end behaviour
// ---------------------------------------------------------------------------

#[test]
fn warmup_period_emits_no_pivots() {
    let mut eng = PivotEngine::new(PivotConfig::defaults()).unwrap();
    for i in 0..10 {
        let b = flat(i, dec!(100) + Decimal::from(i));
        let out = eng.on_bar(&b).unwrap();
        assert!(out.is_empty(), "no pivots before ATR warm-up completes");
    }
}

#[test]
fn synthetic_swing_produces_l0_pivots() {
    // Build a deliberately swingy series so the L0 zigzag has plenty to chew on.
    let cfg = PivotConfig {
        atr_period: 5,
        atr_mult: [dec!(0.5), dec!(1.0), dec!(2.0), dec!(4.0)],
    };
    let mut eng = PivotEngine::new(cfg).unwrap();

    // Up 5, down 5, up 5, down 5 ... over 40 bars.
    let mut price = dec!(100);
    let mut up = true;
    for i in 0..40 {
        if i % 5 == 0 {
            up = !up;
        }
        price += if up { dec!(2) } else { dec!(-2) };
        let b = bar(i, price, price + dec!(1), price - dec!(1), price);
        eng.on_bar(&b).unwrap();
    }
    let tree = eng.snapshot();
    let l0 = tree.at_level(PivotLevel::L0);
    assert!(
        l0.len() >= 3,
        "expected several L0 pivots, got {}",
        l0.len()
    );
    // Pivots must alternate high/low.
    for w in l0.windows(2) {
        assert_ne!(w[0].kind, w[1].kind, "pivots should alternate kind");
    }
}

#[test]
fn higher_levels_are_subsets_of_lower_levels() {
    // Use a tighter L0 threshold and a much wider L3 to force a real
    // hierarchy with cascade emissions on multiple levels.
    let cfg = PivotConfig {
        atr_period: 5,
        atr_mult: [dec!(0.3), dec!(1.0), dec!(2.5), dec!(5.0)],
    };
    let mut eng = PivotEngine::new(cfg).unwrap();

    // Big swings then small swings then big again — should produce L1+ pivots.
    let pattern: Vec<Decimal> = vec![
        dec!(100), dec!(102), dec!(104), dec!(108), dec!(115), dec!(120),
        dec!(118), dec!(112), dec!(105), dec!(100), dec!(95),  dec!(90),
        dec!(92),  dec!(96),  dec!(99),  dec!(103), dec!(110), dec!(118),
        dec!(125), dec!(130), dec!(128), dec!(120), dec!(110), dec!(102),
        dec!(95),  dec!(88),  dec!(82),  dec!(78),  dec!(85),  dec!(94),
        dec!(105), dec!(118), dec!(130), dec!(142), dec!(150),
    ];
    for (i, p) in pattern.iter().enumerate() {
        let b = bar(i as i64, *p, *p + dec!(1), *p - dec!(1), *p);
        eng.on_bar(&b).unwrap();
    }

    let tree = eng.snapshot();
    assert!(
        tree.check_subset_invariant().is_none(),
        "pivot tree must satisfy the subset invariant: {:?}",
        tree.check_subset_invariant()
    );
    assert!(
        tree.count(PivotLevel::L0) >= tree.count(PivotLevel::L1),
        "L0 must have at least as many pivots as L1"
    );
    assert!(tree.count(PivotLevel::L0) > 0, "expected at least one L0 pivot");
}

#[test]
fn rejects_non_monotonic_bars() {
    let mut eng = PivotEngine::new(PivotConfig::defaults()).unwrap();
    eng.on_bar(&bar(10, dec!(100), dec!(101), dec!(99), dec!(100)))
        .unwrap();
    let err = eng
        .on_bar(&bar(5, dec!(100), dec!(101), dec!(99), dec!(100)))
        .unwrap_err();
    assert!(matches!(err, PivotError::NonMonotonic(_)));
}

#[test]
fn snapshot_pivot_kind_alternation_holds_per_level() {
    let cfg = PivotConfig {
        atr_period: 4,
        atr_mult: [dec!(0.5), dec!(1.5), dec!(3.0), dec!(6.0)],
    };
    let mut eng = PivotEngine::new(cfg).unwrap();
    let mut price = dec!(50);
    let mut step = dec!(3);
    for i in 0..30 {
        if i % 4 == 0 {
            step = -step;
        }
        price += step;
        eng.on_bar(&bar(i, price, price + dec!(1), price - dec!(1), price))
            .unwrap();
    }
    let tree = eng.snapshot();
    for level in PivotLevel::ALL {
        let p = tree.at_level(level);
        for w in p.windows(2) {
            assert_ne!(
                w[0].kind, w[1].kind,
                "alternation broken at {:?}: {:?} -> {:?}",
                level, w[0].kind, w[1].kind
            );
        }
    }
}

// Compile-time guard: PivotKind is exported under the path the engine
// builds Pivot values with. Catches accidental rename.
#[test]
fn pivot_kind_is_high_or_low() {
    let _ = PivotKind::High;
    let _ = PivotKind::Low;
}

// Suppress unused-import warning if the helper isn't used by every test
// after future trimming.
#[test]
fn duration_helper_compiles() {
    let _ = Duration::seconds(60);
}
