//! Trendoscope Pine v6 **`Trendoscope/basechartpatterns`** (CC BY-NC-SA) — taşınan parçalar.
//!
//! | Pine | Rust |
//! |------|------|
//! | `getPatternNameById` | [`crate::pattern_name_by_acp_id`](crate::pattern_name_by_acp_id) (`Option`; geçersiz id → `None`, Pine `'Error'`) |
//! | `resolvePatternName` (açı/alan/isExpanding/isContracting/isChannel dalları) | [`resolve_pattern_type_id`] — `bar_diff` Pine’daki `this.trendLine1.p2.index - this.trendLine2.p1.index` (uzatılmış uçlar) |
//! | `resolve` (çizgileri `firstIndex`/`lastIndex`’e `get_price` ile uzatma, pivot fiyatlarını çizgiye projeksiyon) | [`crate::channel_six_drawing_hints`](crate::channel_six_drawing_hints) + `try_scan_six_window` içi uç seçimi |
//! | `find(points, …)` (checkBarRatio, `inspect`, `allowedPatterns`, `allowedLastPivotDirections`; Pine’da `ratioDiff` yalnız Pattern alanı) | [`crate::find::try_scan_six_window`](crate::find::try_scan_six_window) + `scan_six_alternating_pivots`; filtreler [`ChannelSixWindowFilter`](crate::ChannelSixWindowFilter) |
//! | `find(zigzag, …)` (`offset` ile `get(i+offset)`, `avoidOverlap`, duplicate pivot penceresi) | [`crate::analyze_channel_six_from_bars`](crate::analyze_channel_six_from_bars) — `pivot_tail_skip` / seviye döngüsü Zigzag kronolojik dilimde; `offset` birebir aynı parametre adı değil |
//!
//! **Taşınmayan:** `Pattern` nesnesi, `themeColors.shift` döngüsü (renk tema tabloları + batch’te mod), `log.info`, ham `chart.point` dizisi tabanlı `find` imzası — sunucu `OhlcBar` haritası + sonuç `ChannelSixScanOutcome` kullanır.

/// Trend çizgileri uç nokta fiyatları ve `flat_ratio` (Pine `ScanProperties.flatRatio`) ile tip ID.
/// `bar_diff` = `lastIndex - firstIndex` (Pine `resolve` sonrası `trendLine1.p2.index - trendLine2.p1.index`).
#[must_use]
pub fn resolve_pattern_type_id(
    t1p1: f64,
    t1p2: f64,
    t2p1: f64,
    t2p2: f64,
    bar_diff: i64,
    flat_ratio: f64,
) -> i32 {
    if bar_diff == 0 {
        return 0;
    }
    let bd = bar_diff as f64;

    // BUG-4 / Pine parity: `resolvePatternName` uses strict `>` (not `>=`). When `t1p1 == t2p1`,
    // Pine takes the `else` branch; matching `>=` here would diverge pattern_type_id.
    let t1_left_higher = t1p1 > t2p1;

    let upper_angle = if t1_left_higher {
        let den = t1p1 - t2p1.min(t2p2);
        if den.abs() < 1e-15 {
            return 0;
        }
        (t1p2 - t2p1.min(t2p2)) / den
    } else {
        let den = t2p1 - t1p1.min(t1p2);
        if den.abs() < 1e-15 {
            return 0;
        }
        (t2p2 - t1p1.min(t1p2)) / den
    };

    let lower_angle = if t1_left_higher {
        let den = t2p1 - t1p1.max(t1p2);
        if den.abs() < 1e-15 {
            return 0;
        }
        (t2p2 - t1p1.max(t1p2)) / den
    } else {
        let den = t1p1 - t2p1.max(t2p2);
        if den.abs() < 1e-15 {
            return 0;
        }
        (t1p2 - t2p1.max(t2p2)) / den
    };

    let upper_line_dir = if upper_angle > 1.0 + flat_ratio {
        1
    } else if upper_angle < 1.0 - flat_ratio {
        -1
    } else {
        0
    };
    let lower_line_dir = if lower_angle > 1.0 + flat_ratio {
        -1
    } else if lower_angle < 1.0 - flat_ratio {
        1
    } else {
        0
    };

    let start_diff = (t1p1 - t2p1).abs();
    let end_diff = (t1p2 - t2p2).abs();
    let min_diff = start_diff.min(end_diff);
    let price_diff = (start_diff - end_diff).abs() / bd;
    let probable_converging_bars = if price_diff.abs() < 1e-15 {
        f64::INFINITY
    } else {
        min_diff / price_diff
    };

    let is_expanding = (t1p2 - t2p2).abs() > (t1p1 - t2p1).abs();
    let is_contracting = (t1p2 - t2p2).abs() < (t1p1 - t2p1).abs();

    let is_channel = probable_converging_bars > 2.0 * bd
        || (!is_expanding && !is_contracting)
        || (upper_line_dir == 0 && lower_line_dir == 0);

    let invalid = (t1p1 - t2p1).signum() != (t1p2 - t2p2).signum();

    let mut pattern_type = if invalid {
        0
    } else if is_channel {
        if upper_line_dir > 0 && lower_line_dir > 0 {
            1
        } else if upper_line_dir < 0 && lower_line_dir < 0 {
            2
        } else {
            3
        }
    } else if is_expanding {
        if upper_line_dir > 0 && lower_line_dir > 0 {
            4
        } else if upper_line_dir < 0 && lower_line_dir < 0 {
            5
        } else if upper_line_dir > 0 && lower_line_dir < 0 {
            6
        } else if upper_line_dir > 0 && lower_line_dir == 0 {
            7
        } else if upper_line_dir == 0 && lower_line_dir < 0 {
            8
        } else {
            -2
        }
    } else if is_contracting {
        if upper_line_dir > 0 && lower_line_dir > 0 {
            9
        } else if upper_line_dir < 0 && lower_line_dir < 0 {
            10
        } else if upper_line_dir < 0 && lower_line_dir > 0 {
            11
        } else if lower_line_dir == 0 {
            if upper_line_dir < 0 {
                12
            } else {
                1
            }
        } else if upper_line_dir == 0 {
            if lower_line_dir > 0 {
                13
            } else {
                2
            }
        } else {
            -3
        }
    } else {
        -4
    };

    if pattern_type < 0 {
        pattern_type = 0;
    }
    pattern_type
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_sign_returns_zero() {
        // t1 üstte başlayıp altta bitmez — Pine invalid
        let id = resolve_pattern_type_id(100.0, 90.0, 95.0, 105.0, 10, 0.2);
        assert_eq!(id, 0);
    }

    #[test]
    fn equal_left_anchor_falls_into_else_branch_in_pine_parity_mode() {
        // Pine parity: `t1p1 > t2p1 ? ... : ...` => equality falls into else branch.
        // In this setup the else-branch denominator can collapse to ~0, yielding id 0.
        let id = resolve_pattern_type_id(100.0, 110.0, 100.0, 95.0, 10, 0.2);
        assert_eq!(id, 0);
    }
}
