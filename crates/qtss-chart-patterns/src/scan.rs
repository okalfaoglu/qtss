//! Trendoscope Pine v6 **`Trendoscope/abstractchartpatterns`** (CC BY-NC-SA) — taşınan parçaların eşlemesi.
//!
//! | Pine | Rust |
//! |------|------|
//! | `inRange` | [`in_range`] |
//! | `checkBarRatio(p1,p2,p3, …)` | [`check_bar_ratio`] |
//! | `getRatioDiff` | [`get_ratio_diff`] |
//! | `wr.Line.inspect` (iç `get_price` + skor) | [`trend_line_inspect`] |
//! | `array<chart.point>.inspect` (3 nokta, en iyi pick) | [`inspect_pick_best_three_point_line`] |
//! | 2 nokta üst/alt | [`inspect_two_point_line`] |
//!
//! **Bu dosyada yok (başka yerde / yok):**
//! - `DrawingProperties`, `Pattern` (çizgi/polyline/label nesneleri) → [`crate::PatternDrawingBatch`](crate::PatternDrawingBatch) + web.
//! - `ScanProperties` tam tipi → API gövdesi + [`crate::SixPivotScanParams`](crate::SixPivotScanParams) + [`crate::ChannelSixWindowFilter`](crate::ChannelSixWindowFilter).
//! - `SizeFilters.checkSize` → [`crate::find::check_size_pivots`](crate::find::check_size_pivots).
//! - `Pattern.draw` / `erase` / `push` / `deepcopy` → durumsuz tarama; çizim JSON ve istemci.
//!
//! `bar_index` uzayı Pine `chart.point.index` ile aynıdır.

use std::collections::BTreeMap;

use crate::line_price_at_bar_index;
use crate::ohlc::OhlcBar;

#[inline]
#[must_use]
pub fn in_range(value: f64, min: f64, max: f64) -> bool {
    value >= min && value <= max
}

/// Pine `checkBarRatio(p1,p2,p3, properties)`.
#[must_use]
pub fn check_bar_ratio(
    p1_bar: i64,
    p2_bar: i64,
    p3_bar: i64,
    enabled: bool,
    bar_ratio_limit: f64,
) -> bool {
    if !enabled {
        return true;
    }
    let den = (p2_bar - p1_bar).abs() as f64;
    if den == 0.0 {
        return false;
    }
    let r = (p3_bar - p2_bar).abs() as f64 / den;
    in_range(r, bar_ratio_limit, 1.0 / bar_ratio_limit)
}

/// Pine `getRatioDiff(p1,p2,p3)`.
#[must_use]
pub fn get_ratio_diff(p1: (i64, f64), p2: (i64, f64), p3: (i64, f64)) -> Option<f64> {
    let d1 = (p2.0 - p1.0) as f64;
    let d2 = (p3.0 - p2.0) as f64;
    if d1 == 0.0 || d2 == 0.0 {
        return None;
    }
    let first_ratio = (p2.1 - p1.1) / d1;
    let second_ratio = (p3.1 - p2.1) / d2;
    Some((first_ratio - second_ratio).abs())
}

/// Pine `wr.Line.inspect(...)` — tek trend doğrusu için skor ve geçerlilik.
/// Dönüş: `(ok, score)` — `ok == valid && score/total < score_ratio_max` (Pine: `errorThresold/100`, varsayılan 0.2).
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn trend_line_inspect(
    p1: (i64, f64),
    p2: (i64, f64),
    starting_bar: i64,
    ending_bar: i64,
    other_bar: i64,
    direction: f64,
    bars: &BTreeMap<i64, OhlcBar>,
    score_ratio_max: f64,
) -> (bool, f64) {
    if starting_bar > ending_bar {
        return (false, 0.0);
    }
    let mut score = 0.0_f64;
    let mut total = 0.0_f64;
    let mut loop_valid = true;
    let mut b = starting_bar;
    while b <= ending_bar {
        let Some(ohlc) = bars.get(&b) else {
            loop_valid = false;
            break;
        };
        total += 1.0;
        let Some(line_price) = line_price_at_bar_index(p1.0, p1.1, p2.0, p2.1, b) else {
            loop_valid = false;
            break;
        };
        let bar_price = if direction > 0.0 { ohlc.high } else { ohlc.low };
        let bar_out = if direction > 0.0 { ohlc.low } else { ohlc.high };
        let min_oc_dir = (ohlc.open * direction).min(ohlc.close * direction);
        if line_price * direction < min_oc_dir {
            loop_valid = false;
            break;
        }
        if line_price * direction >= bar_out * direction && line_price * direction <= bar_price * direction {
            score += 1.0;
        } else if b == other_bar {
            loop_valid = false;
            break;
        }
        b += 1;
    }
    let ok = loop_valid && total > 0.0 && score / total < score_ratio_max;
    (ok, score)
}

/// Pine `array<chart.point>.inspect` — üç pivot için en iyi segment seçimi (1=uçtan uça, 2=0-1, 3=1-2).
#[must_use]
pub fn inspect_pick_best_three_point_line(
    p0: (i64, f64),
    p1: (i64, f64),
    p2: (i64, f64),
    direction: f64,
    bars: &BTreeMap<i64, OhlcBar>,
    score_ratio_max: f64,
) -> (bool, u8, f64) {
    let first_index = p0.0.min(p1.0).min(p2.0);
    let last_index = p0.0.max(p1.0).max(p2.0);
    let (ok1, s1) = trend_line_inspect(p0, p2, first_index, last_index, p1.0, direction, bars, score_ratio_max);
    let (ok2, s2) = trend_line_inspect(p0, p1, first_index, last_index, p2.0, direction, bars, score_ratio_max);
    let (ok3, s3) = trend_line_inspect(p1, p2, first_index, last_index, p0.0, direction, bars, score_ratio_max);
    let pick = if ok1 && s1 > s2.max(s3) {
        1_u8
    } else if ok2 && s2 > s1.max(s3) {
        2
    } else {
        3
    };
    let final_ok = match pick {
        1 => ok1,
        2 => ok2,
        _ => ok3,
    };
    let final_score = match pick {
        1 => s1,
        2 => s2,
        _ => s3,
    };
    (final_ok, pick, final_score)
}

/// İki pivotlu çizgi: Pine `points.size()==2` dalı — `other` = ilk noktanın barı.
#[must_use]
pub fn inspect_two_point_line(
    p_first: (i64, f64),
    p_last: (i64, f64),
    direction: f64,
    bars: &BTreeMap<i64, OhlcBar>,
    score_ratio_max: f64,
) -> (bool, f64) {
    let first_index = p_first.0.min(p_last.0);
    let last_index = p_first.0.max(p_last.0);
    trend_line_inspect(
        p_first,
        p_last,
        first_index,
        last_index,
        p_first.0,
        direction,
        bars,
        score_ratio_max,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(i: i64, o: f64, h: f64, l: f64, c: f64) -> (i64, OhlcBar) {
        (
            i,
            OhlcBar {
                open: o,
                high: h,
                low: l,
                close: c,
                bar_index: i,
            },
        )
    }

    #[test]
    fn check_bar_ratio_matches_pine_midpoint() {
        // r = |3-2|/|2-1| = 1, limit 0.382 .. 1/0.382
        assert!(check_bar_ratio(0, 2, 4, true, 0.382));
        assert!(!check_bar_ratio(0, 2, 10, true, 0.382));
    }

    #[test]
    fn get_ratio_diff_parallel_slopes() {
        // (0,0)->(1,1)->(2,2) => ratios both 1, diff 0
        let d = get_ratio_diff((0, 0.0), (1, 1.0), (2, 2.0)).unwrap();
        assert!(d < 1e-9);
    }

    #[test]
    fn inspect_line_outside_body_low_score_ratio() {
        // Çizgi tüm mumların üstünde; skor 0 → score/total < 0.2.
        // `otherBar` döngüdeki bir indeksle çakışmamalı: Pine’da o mumda çizgi band dışındaysa valid kırılır.
        let mut m = BTreeMap::new();
        for i in 0..5 {
            let (k, v) = bar(i, 85.0, 90.0, 80.0, 85.0);
            m.insert(k, v);
        }
        let (ok, sc) = trend_line_inspect((0, 100.0), (4, 100.0), 1, 4, 0, 1.0, &m, 0.2);
        assert!(ok);
        assert_eq!(sc, 0.0);
    }

    #[test]
    fn inspect_line_inside_all_bars_high_score_fails() {
        let mut m = BTreeMap::new();
        for i in 0..5 {
            let (k, v) = bar(i, 99.0, 101.0, 99.0, 99.0);
            m.insert(k, v);
        }
        let (ok, sc) = trend_line_inspect((0, 100.0), (4, 100.0), 0, 4, 0, 1.0, &m, 0.2);
        assert!(!ok);
        assert_eq!(sc, 5.0);
    }
}
