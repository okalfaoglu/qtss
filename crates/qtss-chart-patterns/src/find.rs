//! Pine `basechartpatterns.find(…)` → `Pattern.resolve(…)` akışının özü: zigzag / noktalar → iki `inspect` →
//! **`resolve`** ile çizgilerin `firstIndex`/`lastIndex`’e uzatılması → `resolvePatternName` (`resolve_pattern_type_id`).
//!
//! Tam `find(zigzag, patterns[], …)` çoklu formasyon dizisi henüz yok; kanal altılı (veya beşli) penceresi burada taranır.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::line_price_at_bar_index;
use crate::ohlc::OhlcBar;
use crate::resolve::resolve_pattern_type_id;
use crate::scan::{check_bar_ratio, inspect_pick_best_three_point_line, inspect_two_point_line};
use crate::zigzag::{next_level_from_zigzag, pivots_chronological, ZigzagLite, ZigzagPivot};

/// `BTreeMap` bar indeksine göre sıralı OHLC → zigzag.
#[must_use]
pub fn zigzag_from_ohlc_bars(
    bars: &BTreeMap<i64, OhlcBar>,
    zigzag_length: usize,
    max_pivots: usize,
    offset: usize,
) -> ZigzagLite {
    let n = bars.len();
    if n == 0 {
        return ZigzagLite::new(zigzag_length, max_pivots, offset);
    }
    let mut keys: Vec<i64> = bars.keys().copied().collect();
    keys.sort_unstable();
    let mut highs = Vec::with_capacity(n);
    let mut lows = Vec::with_capacity(n);
    let mut times = Vec::with_capacity(n);
    for k in &keys {
        let b = &bars[k];
        highs.push(b.high);
        lows.push(b.low);
        times.push(*k * 60_000); // zaman yoksa bar_index’ten tekil ms (test / önizleme)
    }
    let mut zz = ZigzagLite::new(zigzag_length, max_pivots, offset);
    for (i, &bar_index) in keys.iter().enumerate() {
        zz.calculate_bar(bar_index, i, &highs, &lows, &times);
    }
    zz
}

/// Kronolojik pivot: `(bar_index, price, dir)`.
pub type PivotTriple = (i64, f64, i32);

fn endpoints_from_pick_three(
    pick: u8,
    a: (i64, f64),
    b: (i64, f64),
    c: (i64, f64),
) -> ((i64, f64), (i64, f64)) {
    match pick {
        1 => (a, c),
        2 => (a, b),
        _ => (b, c),
    }
}

/// Altı alterne pivot (H,L,H,L,H,L veya L,H,…) + OHLC haritası → `inspect` + `resolve`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SixPivotScanResult {
    pub pattern_type_id: i32,
    pub pick_upper: u8,
    pub pick_lower: u8,
    pub upper_ok: bool,
    pub lower_ok: bool,
    pub upper_score: f64,
    pub lower_score: f64,
}

/// Pine `abstractchartpatterns.SizeFilters` — pivot penceresi bar genişliği ve fiyat oranı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeFilters {
    #[serde(default)]
    pub filter_by_bar: bool,
    #[serde(default)]
    pub min_pattern_bars: i64,
    #[serde(default = "default_max_pattern_bars")]
    pub max_pattern_bars: i64,
    #[serde(default)]
    pub filter_by_percent: bool,
    #[serde(default)]
    pub min_pattern_percent: f64,
    #[serde(default = "default_max_pattern_percent")]
    pub max_pattern_percent: f64,
}

fn default_max_pattern_bars() -> i64 {
    1000
}

fn default_max_pattern_percent() -> f64 {
    100.0
}

impl Default for SizeFilters {
    fn default() -> Self {
        Self {
            filter_by_bar: false,
            min_pattern_bars: 0,
            max_pattern_bars: 1000,
            filter_by_percent: false,
            min_pattern_percent: 0.0,
            max_pattern_percent: 100.0,
        }
    }
}

/// Pine `SizeFilters.checkSize` — `barDiff = max(index)-min(index)`, `priceDiff = max(price)-min(price)`,
/// `price_ratio = priceDiff / max(|prices|)`; `filterByBar` / `filterByPercent` koşulları aynı mantıkta birleştirilir.
#[must_use]
pub fn check_size_pivots(pivots: &[PivotTriple], filters: &SizeFilters) -> bool {
    if !filters.filter_by_bar && !filters.filter_by_percent {
        return true;
    }
    if pivots.is_empty() {
        return false;
    }
    let mut bar_min = pivots[0].0;
    let mut bar_max = pivots[0].0;
    let mut pr_min = pivots[0].1;
    let mut pr_max = pivots[0].1;
    for (b, pr, _) in pivots {
        bar_min = bar_min.min(*b);
        bar_max = bar_max.max(*b);
        pr_min = pr_min.min(*pr);
        pr_max = pr_max.max(*pr);
    }
    let bar_diff = bar_max - bar_min;
    let price_diff = pr_max - pr_min;
    let denom = pr_max.abs();
    let price_ratio = if denom < 1e-15 {
        0.0
    } else {
        price_diff / denom
    };

    let mut ok = true;
    if filters.filter_by_bar {
        ok = ok && bar_diff >= filters.min_pattern_bars && bar_diff <= filters.max_pattern_bars;
    }
    if filters.filter_by_percent {
        ok = ok
            && price_ratio >= filters.min_pattern_percent
            && price_ratio <= filters.max_pattern_percent;
    }
    ok
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SixPivotScanParams {
    pub number_of_pivots: usize,
    pub bar_ratio_enabled: bool,
    pub bar_ratio_limit: f64,
    pub flat_ratio: f64,
    /// Pine `errorThresold/100` — `inspect` için `score/total < error_score_ratio_max`.
    pub error_score_ratio_max: f64,
    /// Üst sınır çizgisi: `direction > 0` → `trend_line_inspect` yüksekleri kullanır.
    pub upper_direction: f64,
    /// Alt sınır çizgisi: genelde `< 0` (dip).
    pub lower_direction: f64,
    #[serde(default)]
    pub size_filters: SizeFilters,
    /// Pine `ScanProperties.ignoreIfEntryCrossed` — son mum kapanışı üst/alt çizgi bandının dışındaysa elenir.
    #[serde(default)]
    pub ignore_if_entry_crossed: bool,
}

impl Default for SixPivotScanParams {
    fn default() -> Self {
        Self {
            number_of_pivots: 5,
            bar_ratio_enabled: true,
            bar_ratio_limit: 0.382,
            flat_ratio: 0.2,
            error_score_ratio_max: 0.2,
            upper_direction: 1.0,
            lower_direction: -1.0,
            size_filters: SizeFilters::default(),
            ignore_if_entry_crossed: false,
        }
    }
}

/// Pine `find(zigzag, avoidOverlap, …)` ile hizalı isteğe bağlı pencere filtreleri.
#[derive(Debug, Clone, Copy)]
pub struct ChannelSixWindowFilter<'a> {
    pub avoid_overlap: bool,
    /// Mevcut formasyonlar: `(first_bar, last_bar)` — Pine `startBar`/`endBar` (eskiden yeniye).
    pub existing_ranges: &'a [(i64, i64)],
    /// Pine `existingPattern`: 6 pivotlu pencerede ilk 5 pivotun `bar_index` dizisi.
    pub duplicate_pivot_bars: Option<&'a [i64]>,
    /// Pine `allowedLastPivotDirections`: indeks = `pattern_type_id` (1..=13); `0` = filtresiz; `±1` = son pivot fiyat yönü.
    pub allowed_last_pivot_directions: Option<&'a [i32]>,
}

impl<'a> Default for ChannelSixWindowFilter<'a> {
    fn default() -> Self {
        Self {
            avoid_overlap: true,
            existing_ranges: &[],
            duplicate_pivot_bars: None,
            allowed_last_pivot_directions: None,
        }
    }
}

/// `pivots` uzunluğu 6 olmalı; `bars` tüm ilgili aralığı kapsamalı.
#[must_use]
pub fn scan_six_alternating_pivots(
    pivots: &[PivotTriple],
    bars: &BTreeMap<i64, OhlcBar>,
    params: &SixPivotScanParams,
) -> Option<SixPivotScanResult> {
    let n = params.number_of_pivots;
    if !(n == 5 || n == 6) || pivots.len() != n {
        return None;
    }
    let p: Vec<_> = pivots.to_vec();
    // Alternans: dir işareti sırayla değişmeli
    for w in p.windows(2) {
        let (a, b) = (w[0].2.signum(), w[1].2.signum());
        if a == 0 || b == 0 || a == b {
            return None;
        }
    }

    let (b0, pr0) = (p[0].0, p[0].1);
    let (b1, pr1) = (p[1].0, p[1].1);
    let (b2, pr2) = (p[2].0, p[2].1);
    let (b3, pr3) = (p[3].0, p[3].1);
    let (b4, pr4) = (p[4].0, p[4].1);
    let (b5, pr5) = if n == 6 { (p[5].0, p[5].1) } else { (0, 0.0) };

    if !check_bar_ratio(b0, b2, b4, params.bar_ratio_enabled, params.bar_ratio_limit) {
        return None;
    }
    if n == 6 && !check_bar_ratio(b1, b3, b5, params.bar_ratio_enabled, params.bar_ratio_limit) {
        return None;
    }

    let (upper_ok, pick_u, upper_score) = inspect_pick_best_three_point_line(
        (b0, pr0),
        (b2, pr2),
        (b4, pr4),
        params.upper_direction,
        bars,
        params.error_score_ratio_max,
    );
    let (lower_ok, pick_l, lower_score, l1, l2) = if n == 6 {
        let (ok, pick, score) = inspect_pick_best_three_point_line(
            (b1, pr1),
            (b3, pr3),
            (b5, pr5),
            params.lower_direction,
            bars,
            params.error_score_ratio_max,
        );
        let (a, b) = endpoints_from_pick_three(pick, (b1, pr1), (b3, pr3), (b5, pr5));
        (ok, pick, score, a, b)
    } else {
        let (ok, score) = inspect_two_point_line(
            (b1, pr1),
            (b3, pr3),
            params.lower_direction,
            bars,
            params.error_score_ratio_max,
        );
        (ok, 1_u8, score, (b1, pr1), (b3, pr3))
    };

    if !upper_ok || !lower_ok {
        return None;
    }

    let (u1, u2) = endpoints_from_pick_three(pick_u, (b0, pr0), (b2, pr2), (b4, pr4));
    // Pine `Pattern.resolve`: her iki trend çizgisi `firstIndex`/`lastIndex`’te `get_price` — sonra `resolvePatternName`.
    let b_first = if n == 6 { b0.min(b5) } else { b0.min(b4) };
    let b_last = if n == 6 { b0.max(b5) } else { b0.max(b4) };
    let bar_diff = b_last - b_first;
    let (t1p1, t1p2, t2p1, t2p2) = match (
        line_price_at_bar_index(u1.0, u1.1, u2.0, u2.1, b_first),
        line_price_at_bar_index(u1.0, u1.1, u2.0, u2.1, b_last),
        line_price_at_bar_index(l1.0, l1.1, l2.0, l2.1, b_first),
        line_price_at_bar_index(l1.0, l1.1, l2.0, l2.1, b_last),
    ) {
        (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
        _ => return None,
    };
    let pattern_type_id = resolve_pattern_type_id(t1p1, t1p2, t2p1, t2p2, bar_diff, params.flat_ratio);

    Some(SixPivotScanResult {
        pattern_type_id,
        pick_upper: pick_u,
        pick_lower: pick_l,
        upper_ok,
        lower_ok,
        upper_score,
        lower_score,
    })
}

/// Zigzag pivotlarından (en eski önce) son ardışık 6’lıyı alır.
#[must_use]
pub fn last_six_pivots_chrono(zz: &ZigzagLite) -> Option<Vec<PivotTriple>> {
    let ch: Vec<&ZigzagPivot> = pivots_chronological(zz);
    if ch.len() < 6 {
        return None;
    }
    let slice = &ch[ch.len() - 6..];
    Some(
        slice
            .iter()
            .map(|p| (p.point.index, p.point.price, p.dir))
            .collect(),
    )
}

#[must_use]
#[allow(dead_code)]
pub fn last_n_pivots_chrono(zz: &ZigzagLite, n: usize) -> Option<Vec<PivotTriple>> {
    let ch: Vec<&ZigzagPivot> = pivots_chronological(zz);
    if ch.len() < n {
        return None;
    }
    let slice = &ch[ch.len() - n..];
    Some(slice.iter().map(|p| (p.point.index, p.point.price, p.dir)).collect())
}

/// Zigzag → son 6 pivot → `scan_six_alternating_pivots` (hepsi uyuyorsa).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSixScanOutcome {
    pub scan: SixPivotScanResult,
    /// Kullanılan 6 pivot, **eskiden yeniye** (zaman sırası).
    pub pivots: Vec<PivotTriple>,
    /// Zigzag listesindeki toplam pivot sayısı (teşhis).
    pub zigzag_pivot_count: usize,
    /// Pine `ScanProperties.offset`: en yeni `skip` pivot atlanarak alınan 6’lı (0 = yalnızca en güncel pencere).
    #[serde(default)]
    pub pivot_tail_skip: usize,
    /// Çoklu seviye taramada eşleşmenin bulunduğu zigzag seviyesi (`0` = temel seviye).
    #[serde(default)]
    pub zigzag_level: i32,
}

/// Kanal üst/alt doğrusu uçları — web grafik `bar_index` → `open_time` eşler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelLineEndpoint {
    pub bar_index: i64,
    pub price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSixDrawingHints {
    pub upper: [ChannelLineEndpoint; 2],
    pub lower: [ChannelLineEndpoint; 2],
}

/// `inspect` seçimine göre üst/alt trend doğrusu uçları — Pine `resolve()` gibi **formasyon ilk/son barına** uzatılır.
#[must_use]
pub fn channel_six_drawing_hints(outcome: &ChannelSixScanOutcome) -> ChannelSixDrawingHints {
    let p = &outcome.pivots;
    if p.len() == 5 {
        let (b0, pr0) = (p[0].0, p[0].1);
        let (b1, pr1) = (p[1].0, p[1].1);
        let (b2, pr2) = (p[2].0, p[2].1);
        let (b3, pr3) = (p[3].0, p[3].1);
        let (b4, pr4) = (p[4].0, p[4].1);
        let b_first = b0.min(b4);
        let b_last = b0.max(b4);
        let s = &outcome.scan;
        let (u1, u2) = endpoints_from_pick_three(s.pick_upper, (b0, pr0), (b2, pr2), (b4, pr4));
        let (l1, l2) = ((b1, pr1), (b3, pr3));
        let up_a = line_price_at_bar_index(u1.0, u1.1, u2.0, u2.1, b_first).unwrap_or(u1.1);
        let up_b = line_price_at_bar_index(u1.0, u1.1, u2.0, u2.1, b_last).unwrap_or(u2.1);
        let lo_a = line_price_at_bar_index(l1.0, l1.1, l2.0, l2.1, b_first).unwrap_or(l1.1);
        let lo_b = line_price_at_bar_index(l1.0, l1.1, l2.0, l2.1, b_last).unwrap_or(l2.1);
        return ChannelSixDrawingHints {
            upper: [
                ChannelLineEndpoint {
                    bar_index: b_first,
                    price: up_a,
                },
                ChannelLineEndpoint {
                    bar_index: b_last,
                    price: up_b,
                },
            ],
            lower: [
                ChannelLineEndpoint {
                    bar_index: b_first,
                    price: lo_a,
                },
                ChannelLineEndpoint {
                    bar_index: b_last,
                    price: lo_b,
                },
            ],
        };
    }
    let (b0, pr0) = (p[0].0, p[0].1);
    let (b1, pr1) = (p[1].0, p[1].1);
    let (b2, pr2) = (p[2].0, p[2].1);
    let (b3, pr3) = (p[3].0, p[3].1);
    let (b4, pr4) = (p[4].0, p[4].1);
    let (b5, pr5) = (p[5].0, p[5].1);
    let b_first = b0.min(b5);
    let b_last = b0.max(b5);
    let s = &outcome.scan;
    let (u1, u2) = endpoints_from_pick_three(s.pick_upper, (b0, pr0), (b2, pr2), (b4, pr4));
    let (l1, l2) = endpoints_from_pick_three(s.pick_lower, (b1, pr1), (b3, pr3), (b5, pr5));
    let up_a = line_price_at_bar_index(u1.0, u1.1, u2.0, u2.1, b_first).unwrap_or(u1.1);
    let up_b = line_price_at_bar_index(u1.0, u1.1, u2.0, u2.1, b_last).unwrap_or(u2.1);
    let lo_a = line_price_at_bar_index(l1.0, l1.1, l2.0, l2.1, b_first).unwrap_or(l1.1);
    let lo_b = line_price_at_bar_index(l1.0, l1.1, l2.0, l2.1, b_last).unwrap_or(l2.1);
    ChannelSixDrawingHints {
        upper: [
            ChannelLineEndpoint {
                bar_index: b_first,
                price: up_a,
            },
            ChannelLineEndpoint {
                bar_index: b_last,
                price: up_b,
            },
        ],
        lower: [
            ChannelLineEndpoint {
                bar_index: b_first,
                price: lo_a,
            },
            ChannelLineEndpoint {
                bar_index: b_last,
                price: lo_b,
            },
        ],
    }
}

/// Kronolojik pivotlardan en yeniyi `tail_skip` adım geriden başlayarak 6’lı dilim.
#[must_use]
pub fn six_pivots_chrono_tail_skip(zz: &ZigzagLite, tail_skip: usize) -> Option<Vec<PivotTriple>> {
    let ch: Vec<&ZigzagPivot> = pivots_chronological(zz);
    if ch.len() < 6 + tail_skip {
        return None;
    }
    let start = ch.len() - 6 - tail_skip;
    let end = ch.len() - tail_skip;
    Some(
        ch[start..end]
            .iter()
            .map(|p| (p.point.index, p.point.price, p.dir))
            .collect(),
    )
}

#[must_use]
pub fn n_pivots_chrono_tail_skip(zz: &ZigzagLite, n: usize, tail_skip: usize) -> Option<Vec<PivotTriple>> {
    let ch: Vec<&ZigzagPivot> = pivots_chronological(zz);
    if ch.len() < n + tail_skip {
        return None;
    }
    let start = ch.len() - n - tail_skip;
    let end = ch.len() - tail_skip;
    Some(ch[start..end].iter().map(|p| (p.point.index, p.point.price, p.dir)).collect())
}

fn try_scan_six_window(
    six: &[PivotTriple],
    bars: &BTreeMap<i64, OhlcBar>,
    scan_params: &SixPivotScanParams,
    allowed_pattern_ids: Option<&[i32]>,
    window_filter: &ChannelSixWindowFilter<'_>,
) -> Result<SixPivotScanResult, ChannelSixReject> {
    let n = scan_params.number_of_pivots;
    if six.len() != n {
        return Err(ChannelSixReject {
            code: ChannelSixRejectCode::InsufficientPivots,
            have_pivots: Some(six.len()),
            need_pivots: Some(n),
        });
    }
    let p: Vec<_> = six.to_vec();
    for w in p.windows(2) {
        let (a, b) = (w[0].2.signum(), w[1].2.signum());
        if a == 0 || b == 0 || a == b {
            return Err(ChannelSixReject {
                code: ChannelSixRejectCode::PivotAlternation,
                have_pivots: None,
                need_pivots: None,
            });
        }
    }

    let (b0, pr0) = (p[0].0, p[0].1);
    let (b1, pr1) = (p[1].0, p[1].1);
    let (b2, pr2) = (p[2].0, p[2].1);
    let (b3, pr3) = (p[3].0, p[3].1);
    let (b4, pr4) = (p[4].0, p[4].1);
    let (b5, pr5) = if n == 6 { (p[5].0, p[5].1) } else { (0, 0.0) };

    if !check_bar_ratio(b0, b2, b4, scan_params.bar_ratio_enabled, scan_params.bar_ratio_limit) {
        return Err(ChannelSixReject {
            code: ChannelSixRejectCode::BarRatioUpper,
            have_pivots: None,
            need_pivots: None,
        });
    }
    if n == 6 && !check_bar_ratio(b1, b3, b5, scan_params.bar_ratio_enabled, scan_params.bar_ratio_limit) {
        return Err(ChannelSixReject {
            code: ChannelSixRejectCode::BarRatioLower,
            have_pivots: None,
            need_pivots: None,
        });
    }

    if !check_size_pivots(&p, &scan_params.size_filters) {
        return Err(ChannelSixReject {
            code: ChannelSixRejectCode::SizeFilter,
            have_pivots: None,
            need_pivots: None,
        });
    }

    // Pine `find` — `avoidOverlap` / `existingPattern` (henüz `inspect` öncesi).
    let current_start = p[0].0;
    if window_filter.avoid_overlap {
        for &(start_bar, end_bar) in window_filter.existing_ranges {
            let lo = start_bar.min(end_bar);
            let hi = start_bar.max(end_bar);
            if current_start > lo && current_start < hi {
                return Err(ChannelSixReject {
                    code: ChannelSixRejectCode::OverlapIgnored,
                    have_pivots: None,
                    need_pivots: None,
                });
            }
        }
    }
    if let Some(prefix) = window_filter.duplicate_pivot_bars {
        if prefix.len() == n.saturating_sub(1) && (0..n.saturating_sub(1)).all(|i| p[i].0 == prefix[i]) {
            return Err(ChannelSixReject {
                code: ChannelSixRejectCode::DuplicatePivotWindow,
                have_pivots: None,
                need_pivots: None,
            });
        }
    }

    let (upper_ok, _, _) = inspect_pick_best_three_point_line(
        (b0, pr0),
        (b2, pr2),
        (b4, pr4),
        scan_params.upper_direction,
        bars,
        scan_params.error_score_ratio_max,
    );
    if !upper_ok {
        return Err(ChannelSixReject {
            code: ChannelSixRejectCode::InspectUpper,
            have_pivots: None,
            need_pivots: None,
        });
    }

    let lower_ok = if n == 6 {
        let (ok, _, _) = inspect_pick_best_three_point_line(
            (b1, pr1),
            (b3, pr3),
            (b5, pr5),
            scan_params.lower_direction,
            bars,
            scan_params.error_score_ratio_max,
        );
        ok
    } else {
        let (ok, _) = inspect_two_point_line(
            (b1, pr1),
            (b3, pr3),
            scan_params.lower_direction,
            bars,
            scan_params.error_score_ratio_max,
        );
        ok
    };
    if !lower_ok {
        return Err(ChannelSixReject {
            code: ChannelSixRejectCode::InspectLower,
            have_pivots: None,
            need_pivots: None,
        });
    }

    let scan = scan_six_alternating_pivots(six, bars, scan_params).ok_or(ChannelSixReject {
        code: ChannelSixRejectCode::InspectLower,
        have_pivots: None,
        need_pivots: None,
    })?;
    if let Some(allowed) = allowed_pattern_ids {
        if !allowed.is_empty() && !allowed.contains(&scan.pattern_type_id) {
            return Err(ChannelSixReject {
                code: ChannelSixRejectCode::PatternNotAllowed,
                have_pivots: None,
                need_pivots: None,
            });
        }
    }

    // Pine `allowedLastPivotDirections.get(patternType)` + `lastDir = sign(last - prev)`.
    if let Some(dirs) = window_filter.allowed_last_pivot_directions {
        let id = scan.pattern_type_id as usize;
        let required = dirs.get(id).copied().unwrap_or(0);
        if required != 0 {
            let (last_pr, prev_pr) = if n == 6 { (pr5, pr4) } else { (pr4, pr3) };
            let last_dir = if last_pr > prev_pr {
                1
            } else if last_pr < prev_pr {
                -1
            } else {
                0
            };
            if last_dir != required {
                return Err(ChannelSixReject {
                    code: ChannelSixRejectCode::LastPivotDirection,
                    have_pivots: None,
                    need_pivots: None,
                });
            }
        }
    }

    if scan_params.ignore_if_entry_crossed {
        let tmp_outcome = ChannelSixScanOutcome {
            scan: scan.clone(),
            pivots: six.to_vec(),
            zigzag_pivot_count: 0,
            pivot_tail_skip: 0,
            zigzag_level: 0,
        };
        let hints = channel_six_drawing_hints(&tmp_outcome);
        let Some(&last_bar) = bars.keys().next_back() else {
            return Err(ChannelSixReject {
                code: ChannelSixRejectCode::EntryNotInChannel,
                have_pivots: None,
                need_pivots: None,
            });
        };
        let Some(last_ohlc) = bars.get(&last_bar) else {
            return Err(ChannelSixReject {
                code: ChannelSixRejectCode::EntryNotInChannel,
                have_pivots: None,
                need_pivots: None,
            });
        };
        let close = last_ohlc.close;
        let u0 = &hints.upper[0];
        let u1e = &hints.upper[1];
        let l0 = &hints.lower[0];
        let l1e = &hints.lower[1];
        let up_at = crate::line_price_at_bar_index(u0.bar_index, u0.price, u1e.bar_index, u1e.price, last_bar);
        let lo_at = crate::line_price_at_bar_index(l0.bar_index, l0.price, l1e.bar_index, l1e.price, last_bar);
        if let (Some(upr), Some(lopr)) = (up_at, lo_at) {
            let band_lo = upr.min(lopr);
            let band_hi = upr.max(lopr);
            if close < band_lo || close > band_hi {
                return Err(ChannelSixReject {
                    code: ChannelSixRejectCode::EntryNotInChannel,
                    have_pivots: None,
                    need_pivots: None,
                });
            }
        } else {
            return Err(ChannelSixReject {
                code: ChannelSixRejectCode::EntryNotInChannel,
                have_pivots: None,
                need_pivots: None,
            });
        }
    }

    Ok(scan)
}

#[must_use]
pub fn try_scan_channel_six_from_bars(
    bars: &BTreeMap<i64, OhlcBar>,
    zigzag_length: usize,
    max_pivots: usize,
    zigzag_offset: usize,
    scan_params: &SixPivotScanParams,
) -> Option<ChannelSixScanOutcome> {
    analyze_channel_six_from_bars(
        bars,
        zigzag_length,
        max_pivots,
        zigzag_offset,
        scan_params,
        0,
        0,
        None,
        &ChannelSixWindowFilter::default(),
        1,
    )
    .outcomes
    .into_iter()
    .next()
}

/// Eşleşme olmasa bile zigzag pivot sayısı ve hangi aşamada elendiği (API / GUI teşhisi).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelSixRejectCode {
    InsufficientPivots,
    PivotAlternation,
    BarRatioUpper,
    BarRatioLower,
    InspectUpper,
    InspectLower,
    PatternNotAllowed,
    /// Pine `avoidOverlap`: yeni formasyon başlangıcı mevcut aralığın içinde.
    OverlapIgnored,
    /// Pine `existingPattern`: ilk 5 pivot barı önceki kayıtla aynı.
    DuplicatePivotWindow,
    /// Pine `allowedLastPivotDirections` son pivot fiyat yönü uyuşmazlığı.
    LastPivotDirection,
    /// Pine `SizeFilters.checkSize`.
    SizeFilter,
    /// Pine `ignoreIfEntryCrossed` — son `close` kanal bandında değil.
    EntryNotInChannel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSixReject {
    pub code: ChannelSixRejectCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub have_pivots: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub need_pivots: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSixAnalyzeResult {
    pub bar_count: usize,
    pub zigzag_pivot_count: usize,
    /// Bir istekte birden fazla pencere eşleşmesi (`max_matches` > 1 veya çakışmasız tarama).
    pub outcomes: Vec<ChannelSixScanOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reject: Option<ChannelSixReject>,
}

#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn analyze_channel_six_from_bars(
    bars: &BTreeMap<i64, OhlcBar>,
    zigzag_length: usize,
    max_pivots: usize,
    zigzag_offset: usize,
    scan_params: &SixPivotScanParams,
    pivot_tail_skip_max: usize,
    max_zigzag_levels: usize,
    allowed_pattern_ids: Option<&[i32]>,
    window_filter: &ChannelSixWindowFilter<'_>,
    max_matches: usize,
) -> ChannelSixAnalyzeResult {
    let bar_count = bars.len();
    if bar_count == 0 {
        return ChannelSixAnalyzeResult {
            bar_count: 0,
            zigzag_pivot_count: 0,
            outcomes: Vec::new(),
            reject: Some(ChannelSixReject {
                code: ChannelSixRejectCode::InsufficientPivots,
                have_pivots: Some(0),
                need_pivots: Some(scan_params.number_of_pivots),
            }),
        };
    }

    let max_matches = max_matches.clamp(1, 32);
    let mut overlap_ranges: Vec<(i64, i64)> = window_filter.existing_ranges.to_vec();
    let mut outcomes: Vec<ChannelSixScanOutcome> = Vec::new();

    let mut zz = zigzag_from_ohlc_bars(bars, zigzag_length, max_pivots, zigzag_offset);
    let zigzag_pivot_count = zz.pivots.len();
    let mut last_reject = ChannelSixReject {
        code: ChannelSixRejectCode::InspectLower,
        have_pivots: None,
        need_pivots: None,
    };
    // Pine `getZigzagAndPattern`: `while(mlzigzag.zigzagPivots.size() >= 6+offset)` — gösterge `ScanProperties` ilk `offset=0`.
    // 5 pivotlu desen seçili olsa bile döngüye girmek için en az **6** zigzag pivotu gerekir (Trendoscope ACP v6).
    let pine_zz_floor: usize = 6;
    // BUG-7 / Pine parity: `max_zigzag_levels == 0` means unlimited `nextlevel` passes (Pine `while` until pivot floor).
    // Non-zero caps iterations to `0..=max_zigzag_levels` inclusive (same count as prior `level_iter` logic).
    let max_level_inclusive = if max_zigzag_levels == 0 {
        usize::MAX
    } else {
        max_zigzag_levels
    };
    'levels: for _ in 0..=max_level_inclusive {
        let ch = pivots_chronological(&zz);
        let need = scan_params.number_of_pivots;
        let min_zz = pine_zz_floor.max(need);
        if ch.len() < min_zz {
            last_reject = ChannelSixReject {
                code: ChannelSixRejectCode::InsufficientPivots,
                have_pivots: Some(zz.pivots.len()),
                need_pivots: Some(min_zz),
            };
        } else {
            let max_skip = pivot_tail_skip_max.min(ch.len().saturating_sub(need));
            for skip in 0..=max_skip {
                let Some(six) = n_pivots_chrono_tail_skip(&zz, need, skip) else {
                    break;
                };
                let dynamic_filter = ChannelSixWindowFilter {
                    avoid_overlap: window_filter.avoid_overlap,
                    existing_ranges: overlap_ranges.as_slice(),
                    duplicate_pivot_bars: window_filter.duplicate_pivot_bars,
                    allowed_last_pivot_directions: window_filter.allowed_last_pivot_directions,
                };
                match try_scan_six_window(
                    &six,
                    bars,
                    scan_params,
                    allowed_pattern_ids,
                    &dynamic_filter,
                ) {
                    Ok(scan) => {
                        let outcome = ChannelSixScanOutcome {
                            scan,
                            pivots: six.clone(),
                            zigzag_pivot_count,
                            pivot_tail_skip: skip,
                            zigzag_level: zz.level,
                        };
                        if max_matches == 1 {
                            return ChannelSixAnalyzeResult {
                                bar_count,
                                zigzag_pivot_count,
                                outcomes: vec![outcome],
                                reject: None,
                            };
                        }
                        outcomes.push(outcome);
                        if window_filter.avoid_overlap {
                            let mn = six.iter().map(|(b, _, _)| *b).min().unwrap_or(0);
                            let mx = six.iter().map(|(b, _, _)| *b).max().unwrap_or(0);
                            overlap_ranges.push((mn, mx));
                        }
                        if outcomes.len() >= max_matches {
                            break 'levels;
                        }
                    }
                    Err(e) => {
                        if e.code == ChannelSixRejectCode::PatternNotAllowed
                            || last_reject.code != ChannelSixRejectCode::PatternNotAllowed
                        {
                            last_reject = e;
                        }
                    }
                }
            }
            // Pine `getZigzagAndPattern`: her `find` sonrası `mlzigzag.nextlevel()` — eşleşme olsa da üst zigzag seviyeleri denenir.
        }
        zz = next_level_from_zigzag(&zz);
        if zz.pivots.len() < scan_params.number_of_pivots {
            break 'levels;
        }
    }

    if outcomes.is_empty() {
        ChannelSixAnalyzeResult {
            bar_count,
            zigzag_pivot_count,
            outcomes,
            reject: Some(last_reject),
        }
    } else {
        ChannelSixAnalyzeResult {
            bar_count,
            zigzag_pivot_count,
            outcomes,
            reject: None,
        }
    }
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
                volume: None,
            },
        )
    }

    #[test]
    fn zigzag_from_map_runs() {
        let mut m = BTreeMap::new();
        for i in 0..20 {
            let h = 100.0 + i as f64 * 0.1;
            let (k, v) = bar(i, h - 1.0, h, h - 2.0, h - 0.5);
            m.insert(k, v);
        }
        let zz = zigzag_from_ohlc_bars(&m, 3, 32, 0);
        assert!(!zz.pivots.is_empty());
    }

    #[test]
    fn scan_six_rejects_bad_input() {
        assert!(scan_six_alternating_pivots(&[], &BTreeMap::new(), &SixPivotScanParams::default()).is_none());
        let p = vec![(0, 1.0, 1), (1, 1.0, 1)]; // aynı yön
        assert!(scan_six_alternating_pivots(&p, &BTreeMap::new(), &SixPivotScanParams::default()).is_none());
    }

    #[test]
    fn try_scan_channel_six_empty_map() {
        assert!(
            try_scan_channel_six_from_bars(&BTreeMap::new(), 3, 32, 0, &SixPivotScanParams::default()).is_none()
        );
    }

    #[test]
    fn analyze_channel_six_reports_insufficient_pivots() {
        let mut m = BTreeMap::new();
        for i in 0..8 {
            let (k, v) = bar(i, 100.0, 101.0, 99.0, 100.5);
            m.insert(k, v);
        }
        let a = analyze_channel_six_from_bars(
            &m,
            5,
            32,
            0,
            &SixPivotScanParams::default(),
            0,
            0,
            None,
            &ChannelSixWindowFilter::default(),
            1,
        );
        assert!(a.outcomes.is_empty());
        let r = a.reject.expect("reject");
        assert_eq!(r.code, ChannelSixRejectCode::InsufficientPivots);
        assert_eq!(a.bar_count, 8);
        assert!(a.zigzag_pivot_count < 6);
    }

    #[test]
    fn six_pivot_synthetic_lines_pass_inspect_and_resolve() {
        // `trend_line_inspect`: skor/total < 0.2 ve `otherBar` mumunda bant içi şart — 5 barlı pencerede tek skor = 1/5 = 0.2 → geçmez.
        // Pivotları seyrek tutup (0..25) çoğu mumda çizgiyi bant dışı bırakıyoruz; yalnızca gerekli barlarda teğet.
        let mut m = BTreeMap::new();
        for i in 0..=25 {
            let (k, v) = if i % 2 == 0 {
                // Tepe mumları: çoğu 99 tepede y=100 çizgisi skorlanmaz; pivot barlarda 100.
                let h = if i == 0 || i == 10 || i == 20 { 100.0 } else { 99.0 };
                bar(i, h - 1.0, h, h - 3.0, h - 0.5)
            } else {
                let lo = if i == 5 || i == 15 || i == 25 { 75.0 } else { 76.0 };
                let hi = lo + 3.0;
                bar(i, lo + 1.0, hi, lo, lo + 1.2)
            };
            m.insert(k, v);
        }
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 75.0, -1),
            (10, 100.0, 1),
            (15, 75.0, -1),
            (20, 100.0, 1),
            (25, 75.0, -1),
        ];
        let params = SixPivotScanParams {
            number_of_pivots: 6,
            bar_ratio_enabled: false,
            bar_ratio_limit: 0.382,
            flat_ratio: 0.2,
            error_score_ratio_max: 0.2,
            upper_direction: 1.0,
            lower_direction: -1.0,
            ..Default::default()
        };
        let r = scan_six_alternating_pivots(&pivots, &m, &params).expect("scan");
        assert!(r.pattern_type_id >= 1);
        assert!(r.upper_ok && r.lower_ok);
    }

    #[test]
    fn six_pivots_chrono_tail_skip_zero_matches_last_six() {
        use crate::zigzag::{ChartPoint, ZigzagLite, ZigzagPivot};

        let mut zz = ZigzagLite::new(3, 32, 0);
        for i in (0..8).rev() {
            let dir = if i % 2 == 0 { 1 } else { -1 };
            zz.pivots.insert(
                0,
                ZigzagPivot::new(
                    ChartPoint {
                        index: i,
                        price: 100.0,
                        time_ms: i,
                    },
                    dir,
                ),
            );
        }
        assert_eq!(
            six_pivots_chrono_tail_skip(&zz, 0).expect("six"),
            last_six_pivots_chrono(&zz).expect("last six")
        );
    }

    #[test]
    fn check_size_rejects_when_bar_span_too_narrow() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (1, 90.0, -1),
            (2, 100.0, 1),
            (3, 90.0, -1),
            (4, 100.0, 1),
            (5, 90.0, -1),
        ];
        let mut f = SizeFilters {
            filter_by_bar: true,
            min_pattern_bars: 10,
            ..SizeFilters::default()
        };
        assert!(!check_size_pivots(&pivots, &f));
        f.min_pattern_bars = 0;
        assert!(check_size_pivots(&pivots, &f));
    }

    #[test]
    fn scan_rejects_when_pattern_not_allowed() {
        let mut m = BTreeMap::new();
        for i in 0..=25 {
            let (k, v) = if i % 2 == 0 {
                let h = if i == 0 || i == 10 || i == 20 { 100.0 } else { 99.0 };
                bar(i, h - 1.0, h, h - 3.0, h - 0.5)
            } else {
                let lo = if i == 5 || i == 15 || i == 25 { 75.0 } else { 76.0 };
                let hi = lo + 3.0;
                bar(i, lo + 1.0, hi, lo, lo + 1.2)
            };
            m.insert(k, v);
        }
        let params = SixPivotScanParams {
            number_of_pivots: 6,
            bar_ratio_enabled: false,
            bar_ratio_limit: 0.382,
            flat_ratio: 0.2,
            error_score_ratio_max: 0.2,
            upper_direction: 1.0,
            lower_direction: -1.0,
            ..Default::default()
        };
        let six: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 75.0, -1),
            (10, 100.0, 1),
            (15, 75.0, -1),
            (20, 100.0, 1),
            (25, 75.0, -1),
        ];
        let err = try_scan_six_window(&six, &m, &params, Some(&[13]), &ChannelSixWindowFilter::default())
            .expect_err("pattern filter reject");
        assert_eq!(err.code, ChannelSixRejectCode::PatternNotAllowed);
    }

}
