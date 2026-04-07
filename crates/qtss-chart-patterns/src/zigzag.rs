//! Pine **ZigzagLite** ile aynı kavramsal model: `PivotCandle` penceresi (`length`),
//! pivot listesi (Pine `array.unshift` → `[0]` en güncel), `calculate` adımı ve basit `nextlevel`.
//!
//! `bar_index` uzayı `chart.point.index` ile uyumludur; seri çalıştırmada genelde `bar_index == idx`.
//!
//! Pine `addnewpivot` / `shift` + `addnewpivot` güncelleme yolu — `reference/trendoscope_zigzag_transcript_excerpt.pine`.

use std::collections::VecDeque;

/// Boyutsuz oran alanları (`ratio`, `bar_ratio`, `size_ratio`): sabit 3 ondalık, çok küçük fiyat rejimlerinde
/// anlamlı farkları sıfırlayabilir; yaklaşık 6 anlamlı basamak kullanılır.
#[inline]
fn snap_ratio(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let ax = x.abs();
    if ax == 0.0 {
        return 0.0;
    }
    let mag = ax.log10().floor();
    let scale = 10f64.powf(5.0 - mag);
    (x * scale).round() / scale
}

/// Grafik noktası (`chart.point` benzeri).
#[derive(Debug, Clone, PartialEq)]
pub struct ChartPoint {
    pub index: i64,
    pub price: f64,
    pub time_ms: i64,
}

/// Zigzag pivotu (yön: `dir > 0` tepe, `dir < 0` dip; `|dir|==2` varyantları Pine’da genişleme sonrası olabilir).
#[derive(Debug, Clone, PartialEq)]
pub struct ZigzagPivot {
    pub point: ChartPoint,
    pub dir: i32,
    pub level: i32,
    pub ratio: f64,
    pub bar_ratio: f64,
    pub size_ratio: f64,
}

impl ZigzagPivot {
    #[must_use]
    pub fn new(point: ChartPoint, dir: i32) -> Self {
        Self {
            point,
            dir,
            level: 0,
            ratio: 0.0,
            bar_ratio: 0.0,
            size_ratio: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZigzagFlags {
    pub new_pivot: bool,
    pub double_pivot: bool,
    pub update_last_pivot: bool,
}

/// Pine `Zigzag` tipi: `length`, `numberOfPivots`, `offset`, çok seviye `level`.
#[derive(Debug, Clone)]
pub struct ZigzagLite {
    pub length: usize,
    pub number_of_pivots: usize,
    pub offset: usize,
    pub level: i32,
    /// Ön uç = en yeni pivot (Pine `unshift`); `VecDeque` ile O(1) başa ekleme.
    pub pivots: VecDeque<ZigzagPivot>,
    pub flags: ZigzagFlags,
}

impl ZigzagLite {
    #[must_use]
    pub fn new(length: usize, number_of_pivots: usize, offset: usize) -> Self {
        Self {
            length: length.max(1),
            number_of_pivots: number_of_pivots.max(1),
            offset,
            level: 0,
            pivots: VecDeque::new(),
            flags: ZigzagFlags::default(),
        }
    }

    fn clear_flags(&mut self) {
        self.flags = ZigzagFlags::default();
    }

    /// Pine `addnewpivot`: alternans, `dir` gerekirse ±2, oranlar, `unshift` + boyut sınırı.
    fn add_new_pivot(&mut self, mut pivot: ZigzagPivot) {
        let incoming_sign = pivot.dir.signum();
        if incoming_sign == 0 {
            return;
        }
        if self.pivots.is_empty() {
            self.pivots.push_front(pivot);
            while self.pivots.len() > self.number_of_pivots {
                self.pivots.pop_back();
            }
            self.flags.new_pivot = true;
            return;
        }
        let last = self.pivots.front().expect("non-empty has front");
        if last.dir.signum() == incoming_sign {
            return;
        }
        if self.pivots.len() >= 2 {
            let llast = &self.pivots[1];
            let dir = incoming_sign;
            let value = pivot.point.price;
            let llast_value = llast.point.price;
            let last_value = last.point.price;
            let doubled = (dir as f64) * value > (dir as f64) * llast_value;
            pivot.dir = if doubled { dir * 2 } else { dir };
            let den_seg = (last_value - llast_value).abs().max(1e-15);
            pivot.ratio = snap_ratio((value - last_value).abs() / den_seg);
            let den_bar = (llast.point.index - last.point.index).abs().max(1);
            pivot.bar_ratio = snap_ratio((last.point.index - pivot.point.index).abs() as f64 / den_bar as f64);
            if self.pivots.len() >= 3 {
                let lllast = &self.pivots[2];
                let den_sz = (llast.point.price - lllast.point.price).abs().max(1e-15);
                pivot.size_ratio = snap_ratio((last_value - value).abs() / den_sz);
            }
        }
        self.pivots.push_front(pivot);
        while self.pivots.len() > self.number_of_pivots {
            self.pivots.pop_back();
        }
        self.flags.new_pivot = true;
    }

    /// Pine `ta.highest` / `ta.highestbars` benzeri: pencere `[i-(L-1)..=i]`, geri sayım 0 = güncel bar.
    #[must_use]
    pub fn pivot_candle(length: usize, i: usize, highs: &[f64], lows: &[f64]) -> (i32, i32, f64, f64) {
        let len = length.max(1);
        let start = i.saturating_sub(len.saturating_sub(1));
        let mut p_high = f64::NEG_INFINITY;
        let mut p_high_back = 0i32;
        let mut p_low = f64::INFINITY;
        let mut p_low_back = 0i32;
        // Pine `ta.highest` / `ta.lowest`: ties pick the **oldest** bar (first in window), not the newest.
        for j in start..=i {
            let h = highs[j];
            if h > p_high {
                p_high = h;
                p_high_back = (i - j) as i32;
            }
            let l = lows[j];
            if l < p_low {
                p_low = l;
                p_low_back = (i - j) as i32;
            }
        }
        (p_high_back, p_low_back, p_high, p_low)
    }

    fn last_trend_dir(last_dir: i32) -> i32 {
        if last_dir > 0 {
            1
        } else {
            -1
        }
    }

    /// Tek bar güncellemesi — Pine `Zigzag.calculate`: `forceDoublePivot` (önce), step 1 güncelleme, step 2 karşı pivot, step 3 taşma.
    pub fn calculate_bar(
        &mut self,
        bar_index: i64,
        idx: usize,
        highs: &[f64],
        lows: &[f64],
        times_ms: &[i64],
    ) {
        self.clear_flags();
        if idx >= highs.len() || idx >= lows.len() {
            return;
        }
        // Pine parity (practical): avoid emitting pivots until the first full pivot window exists.
        // This reduces early-series drift where Pine scripts often behave as if `ta.highest/lowest(length)` is not yet stable.
        if idx + 1 < self.length {
            return;
        }
        let new_bar = bar_index - self.offset as i64;
        let (p_high_bar, p_low_bar, p_high, p_low) = Self::pivot_candle(self.length, idx, highs, lows);

        // İlk pivot: pencerede tepe veya dip teyidi.
        if self.pivots.is_empty() {
            if p_high_bar == 0 {
                let t = times_ms.get(idx).copied().unwrap_or(0);
                self.add_new_pivot(ZigzagPivot::new(
                    ChartPoint {
                        index: new_bar,
                        price: p_high,
                        time_ms: t,
                    },
                    1,
                ));
            } else if p_low_bar == 0 {
                let t = times_ms.get(idx).copied().unwrap_or(0);
                self.add_new_pivot(ZigzagPivot::new(
                    ChartPoint {
                        index: new_bar,
                        price: p_low,
                        time_ms: t,
                    },
                    -1,
                ));
            }
            return;
        }

        let p_dir_before = Self::last_trend_dir(self.pivots.front().expect("checked non-empty").dir);
        let last_ix = self.pivots.front().expect("checked non-empty").point.index;
        let distance = new_bar - last_ix;
        let overflow = distance >= self.length as i64;

        // Pine: `forceDoublePivot` is read from `zigzagPivots.get(1)` **before** step 1 (same-bar double pivot).
        let force_double = if self.pivots.len() > 1 {
            let ll = &self.pivots[1];
            if p_dir_before == 1 && p_low_bar == 0 {
                p_low < ll.point.price
            } else if p_dir_before == -1 && p_high_bar == 0 {
                p_high > ll.point.price
            } else {
                false
            }
        } else {
            false
        };

        // 1) Aynı yönde uç güncelle — Pine: `shift` + `Pivot.new` + `addnewpivot` (yerinde atama değil).
        if (p_dir_before == 1 && p_high_bar == 0) || (p_dir_before == -1 && p_low_bar == 0) {
            let lp = self.pivots.front().expect("non-empty").clone();
            let value = if p_dir_before == 1 { p_high } else { p_low };
            let remove_old = value * f64::from(lp.dir) >= lp.point.price * f64::from(lp.dir);
            if remove_old {
                self.flags.update_last_pivot = true;
                self.pivots.pop_front();
                let t = times_ms.get(idx).copied().unwrap_or(0);
                let base_dir = lp.dir.signum();
                self.add_new_pivot(ZigzagPivot::new(
                    ChartPoint {
                        index: new_bar,
                        price: value,
                        time_ms: t,
                    },
                    base_dir,
                ));
            }
        }

        // Pine `calculate`: adım 2 ve 3 `pDir` ile çalışır; `pDir` adım 1 öncesi `lastPivot` ile atanır,
        // adım 1 sonrası yeniden okunmaz (`05_ZigzagLite.pine`).
        let p_dir = p_dir_before;

        // 2) Karşı pivot (Pine: `not newPivot || forceDouble`; burada adım başında newPivot temiz).
        let allow_opp = !self.flags.new_pivot || force_double;
        if allow_opp {
            // p_dir==1: son tepe tarafındayız → dip ekle; p_dir==-1 → tepe ekle.
            if p_dir == 1 && p_low_bar == 0 {
                let piv_bar_back = p_low_bar;
                let src_idx = idx.saturating_sub(piv_bar_back as usize);
                let piv_bar = new_bar - i64::from(piv_bar_back);
                let t = times_ms.get(src_idx).copied().unwrap_or(0);
                self.add_new_pivot(ZigzagPivot::new(
                    ChartPoint {
                        index: piv_bar,
                        price: p_low,
                        time_ms: t,
                    },
                    -1,
                ));
            } else if p_dir == -1 && p_high_bar == 0 {
                let piv_bar_back = p_high_bar;
                let src_idx = idx.saturating_sub(piv_bar_back as usize);
                let piv_bar = new_bar - i64::from(piv_bar_back);
                let t = times_ms.get(src_idx).copied().unwrap_or(0);
                self.add_new_pivot(ZigzagPivot::new(
                    ChartPoint {
                        index: piv_bar,
                        price: p_high,
                        time_ms: t,
                    },
                    1,
                ));
            }
        }

        // 3) Taşma: uzun süre yeni pivot yoksa zorunlu pivot (Pine `overflow and not newPivot`).
        if overflow && !self.flags.new_pivot {
            let (piv_bar_back, price, dir) = if p_dir == 1 {
                (p_low_bar, p_low, -1)
            } else {
                (p_high_bar, p_high, 1)
            };
            let src_idx = idx.saturating_sub(piv_bar_back as usize);
            let piv_bar = new_bar - i64::from(piv_bar_back);
            let t = times_ms.get(src_idx).copied().unwrap_or(0);
            self.add_new_pivot(ZigzagPivot::new(
                ChartPoint {
                    index: piv_bar,
                    price,
                    time_ms: t,
                },
                dir,
            ));
        }
    }

    /// Tüm diziyi sırayla işler (`bar_index == i` varsayımı).
    #[must_use]
    pub fn run_series(highs: &[f64], lows: &[f64], times_ms: &[i64], length: usize, max_pivots: usize, offset: usize) -> Self {
        let mut z = Self::new(length, max_pivots, offset);
        for i in 0..highs.len().min(lows.len()) {
            z.calculate_bar(i as i64, i, highs, lows, times_ms);
        }
        z
    }
}

/// Üst seviye zigzag: pivot **fiyat** dizisine aynı `pivot_candle` kuralını uygular (Pine `nextlevel` özü).
#[must_use]
pub fn next_level_from_pivot_prices(prices: &[f64], bar_indices: &[i64], times_ms: &[i64], length: usize, max_pivots: usize) -> ZigzagLite {
    let n = prices.len().min(bar_indices.len()).min(times_ms.len());
    if n == 0 {
        return ZigzagLite::new(length, max_pivots, 0);
    }
    let highs: Vec<f64> = prices[..n].to_vec();
    let lows = highs.clone();
    ZigzagLite::run_series(&highs, &lows, &times_ms[..n], length, max_pivots, 0)
}

/// Pine `ZigzagLite.nextlevel` parity:
/// - kronolojik pivot akışını (eski → yeni) yürütür,
/// - `nextLevel` boşken yalnızca `|dir|==2` pivot eklenir; `|dir|==1` atlanır (Pine `else if math.abs(dir)==2`),
/// - doluyken `|dir|==1` pivotları geçici tamponlarda (`tempBullishPivot`/`tempBearishPivot`) tutulur,
/// - `|dir|==2` pivot geldiğinde son pivota göre gerektiğinde `shift` ve/veya temp geçişi eklenir,
/// - ardından pivot eklenir ve temp’ler temizlenir.
#[must_use]
pub fn next_level_from_zigzag(source: &ZigzagLite) -> ZigzagLite {
    let mut next_level = ZigzagLite::new(source.length, source.number_of_pivots, 0);
    next_level.level = source.level + 1;
    let chrono = pivots_chronological(source);
    let mut temp_bullish: Option<ZigzagPivot> = None;
    let mut temp_bearish: Option<ZigzagPivot> = None;

    for p_ref in chrono {
        let mut lp = p_ref.clone();
        lp.level += 1;
        let dir = lp.dir;

        if next_level.pivots.is_empty() {
            // Pine: when `nextLevel.zigzagPivots.size() == 0`, only `math.abs(dir)==2` calls `addnewpivot`.
            // Single-dir pivots are not buffered in temps until at least one major pivot exists.
            if dir.abs() == 2 {
                next_level.add_new_pivot(lp);
                // Pine: temps were never filled while `nextLevel` was empty; keep buffers aligned.
                temp_bullish = None;
                temp_bearish = None;
            }
            continue;
        }

        let new_dir = dir.signum();
        let value = lp.point.price;

        let last_dir = next_level.pivots.front().expect("non-empty after first add").dir.signum();
        let last_value = next_level.pivots.front().expect("non-empty").point.price;

        if dir.abs() == 2 {
            if last_dir == new_dir {
                // Same direction: keep the more extreme pivot; otherwise try inserting opposite temp as a bridge.
                if (dir as f64) * last_value < (dir as f64) * value {
                    next_level.pivots.pop_front();
                } else {
                    let temp = if new_dir > 0 { &temp_bearish } else { &temp_bullish };
                    if let Some(tp) = temp {
                        next_level.add_new_pivot(tp.clone());
                    } else {
                        continue;
                    }
                }
            } else {
                // Direction change: optional temp-first + temp-second bridge insertion.
                let temp_first = if new_dir > 0 { &temp_bullish } else { &temp_bearish };
                let temp_second = if new_dir > 0 { &temp_bearish } else { &temp_bullish };
                if let (Some(tf), Some(ts)) = (temp_first, temp_second) {
                    let temp_val = tf.point.price;
                    if (new_dir as f64) * temp_val > (new_dir as f64) * value {
                        next_level.add_new_pivot(tf.clone());
                        next_level.add_new_pivot(ts.clone());
                    }
                }
            }

            next_level.add_new_pivot(lp);
            temp_bullish = None;
            temp_bearish = None;
        } else {
            // |dir|==1: keep the most extreme candidate in temp buffer.
            let slot = if new_dir > 0 {
                &mut temp_bullish
            } else {
                &mut temp_bearish
            };
            match slot {
                Some(existing) => {
                    if (dir as f64) * value > (dir as f64) * existing.point.price {
                        *slot = Some(lp);
                    }
                }
                None => {
                    *slot = Some(lp);
                }
            }
        }
    }

    // Pine'daki güvenlik: üst seviye alt seviyeden anlamsız biçimde yoğunlaşırsa temizle.
    if next_level.pivots.len() >= source.pivots.len() {
        next_level.pivots.clear();
    }
    next_level
}

/// Pivotları eski → yeni sıraya çevirir (çizim / `inspect` için).
#[must_use]
pub fn pivots_chronological(zz: &ZigzagLite) -> Vec<&ZigzagPivot> {
    let mut v: Vec<_> = zz.pivots.iter().collect();
    v.reverse();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pivot_candle_window_extremes_and_bars_back() {
        let h = [1.0, 2.0, 5.0, 4.0];
        let l = [0.5, 1.0, 2.0, 3.0];
        // length=3, i=3 → pencere bar 1..=3: high [2,5,4] → tepe 5 @ bar 2 → geri sayım 3-2=1
        let (phb, plb, ph, pl) = ZigzagLite::pivot_candle(3, 3, &h, &l);
        assert_eq!(phb, 1);
        assert!((ph - 5.0).abs() < 1e-9);
        assert_eq!(plb, 2);
        assert!((pl - 1.0).abs() < 1e-9);
    }

    #[test]
    fn pivot_candle_equal_highs_keeps_oldest() {
        let h = [5.0, 5.0, 5.0, 3.0];
        let l = [1.0, 1.0, 1.0, 1.0];
        let (phb, _, ph, _) = ZigzagLite::pivot_candle(4, 3, &h, &l);
        assert_eq!(phb, 3);
        assert!((ph - 5.0).abs() < 1e-9);
    }

    #[test]
    fn pivot_candle_zero_bars_back_means_current_bar_holds_extreme() {
        let h = [1.0, 2.0, 3.0, 9.0];
        let l = [0.5, 1.0, 2.0, 8.0];
        let (phb, _, ph, _) = ZigzagLite::pivot_candle(3, 3, &h, &l);
        assert_eq!(phb, 0);
        assert!((ph - 9.0).abs() < 1e-9);

        let h2 = [5.0, 6.0, 7.0, 8.0];
        let l2 = [4.0, 4.0, 4.0, 0.5];
        let (_, plb, _, pl) = ZigzagLite::pivot_candle(3, 3, &h2, &l2);
        assert_eq!(plb, 0);
        assert!((pl - 0.5).abs() < 1e-9);
    }

    #[test]
    fn run_series_alternating_swings() {
        // Yapay tepe-dip: 0..10 yükseliş, sonra düşüş
        let mut h: Vec<f64> = (0..15).map(|i| 10.0 + i as f64).collect();
        let mut l: Vec<f64> = h.iter().map(|&x| x - 0.5).collect();
        // 10..15 düşüş
        for i in 10..15 {
            h[i] = 25.0 - (i - 10) as f64;
            l[i] = h[i] - 0.5;
        }
        let times: Vec<i64> = (0..15).map(|i| i * 60_000).collect();
        let z = ZigzagLite::run_series(&h, &l, &times, 3, 32, 0);
        assert!(!z.pivots.is_empty());
        let ch = pivots_chronological(&z);
        // En az bir tepe ve bir dip sırası
        assert!(ch.len() >= 2);
    }

    #[test]
    fn next_level_runs_on_price_path() {
        let prices = [10.0, 12.0, 9.0, 11.0, 8.0];
        let bars = [0_i64, 1, 2, 3, 4];
        let t = [0_i64, 1, 2, 3, 4];
        let z = next_level_from_pivot_prices(&prices, &bars, &t, 2, 16);
        assert!(!z.pivots.is_empty());
    }

    #[test]
    fn next_level_from_zigzag_uses_only_double_dirs() {
        let mut src = ZigzagLite::new(3, 32, 0);
        // En yeni başta olacak şekilde koyuyoruz.
        src.pivots = VecDeque::from(vec![
            ZigzagPivot::new(
                ChartPoint {
                    index: 4,
                    price: 8.0,
                    time_ms: 4,
                },
                -1,
            ),
            ZigzagPivot::new(
                ChartPoint {
                    index: 3,
                    price: 11.0,
                    time_ms: 3,
                },
                2,
            ),
            ZigzagPivot::new(
                ChartPoint {
                    index: 2,
                    price: 9.0,
                    time_ms: 2,
                },
                -2,
            ),
            ZigzagPivot::new(
                ChartPoint {
                    index: 1,
                    price: 12.0,
                    time_ms: 1,
                },
                1,
            ),
        ]);
        let nl = next_level_from_zigzag(&src);
        assert_eq!(nl.level, 1);
        // Pine parity: nextlevel can carry pivots with |dir|==2 as well; at minimum we expect alternating signs.
        assert!(nl.pivots.len() >= 2);
        let seq: Vec<_> = nl.pivots.iter().collect();
        assert!(seq.windows(2).all(|w| {
            w[0].dir.signum() != 0 && w[1].dir.signum() != 0 && w[0].dir.signum() != w[1].dir.signum()
        }));
    }

    #[test]
    fn next_level_from_zigzag_compresses_same_sign_candidates() {
        let mut src = ZigzagLite::new(3, 32, 0);
        // En yeni başta; kronolojik akışta +2,+2,-2,-2,+2 görülür.
        src.pivots = VecDeque::from(vec![
            ZigzagPivot::new(
                ChartPoint {
                    index: 5,
                    price: 14.0,
                    time_ms: 5,
                },
                2,
            ),
            ZigzagPivot::new(
                ChartPoint {
                    index: 4,
                    price: 8.0,
                    time_ms: 4,
                },
                -2,
            ),
            ZigzagPivot::new(
                ChartPoint {
                    index: 3,
                    price: 9.0,
                    time_ms: 3,
                },
                -2,
            ),
            ZigzagPivot::new(
                ChartPoint {
                    index: 2,
                    price: 13.0,
                    time_ms: 2,
                },
                2,
            ),
            ZigzagPivot::new(
                ChartPoint {
                    index: 1,
                    price: 12.0,
                    time_ms: 1,
                },
                2,
            ),
        ]);
        let nl = next_level_from_zigzag(&src);
        let ch = pivots_chronological(&nl);
        // +2 kümesinden yüksek olan 13, -2 kümesinden düşük olan 8, son +2 = 14
        assert_eq!(ch.len(), 3);
        assert!((ch[0].point.price - 13.0).abs() < 1e-9);
        assert!((ch[1].point.price - 8.0).abs() < 1e-9);
        assert!((ch[2].point.price - 14.0).abs() < 1e-9);
    }

    /// Pine `nextlevel`: while `nextLevel` is empty, `|dir|==1` iterations do nothing (no temp buffers).
    /// Seeding temps in Rust caused stale `tempBullish`/`tempBearish` after the first `|dir|==2` add (empty branch
    /// does not clear temps), so a later bridge could inject spurious pivots — diverging from Pine.
    #[test]
    fn next_level_from_zigzag_pine_no_temp_while_next_level_was_empty() {
        let mut src = ZigzagLite::new(3, 32, 0);
        // Chrono oldest → newest: +1 @100, -1 @80, -2 @50, +2 @90
        src.pivots = VecDeque::from(vec![
            ZigzagPivot::new(ChartPoint { index: 4, price: 90.0, time_ms: 4 }, 2),
            ZigzagPivot::new(ChartPoint { index: 3, price: 50.0, time_ms: 3 }, -2),
            ZigzagPivot::new(ChartPoint { index: 2, price: 80.0, time_ms: 2 }, -1),
            ZigzagPivot::new(ChartPoint { index: 1, price: 100.0, time_ms: 1 }, 1),
        ]);
        let nl = next_level_from_zigzag(&src);
        let ch = pivots_chronological(&nl);
        assert_eq!(ch.len(), 2, "{ch:?}");
        assert!((ch[0].point.price - 50.0).abs() < 1e-9);
        assert!((ch[1].point.price - 90.0).abs() < 1e-9);
    }
}
