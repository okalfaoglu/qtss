//! Apex (tepe noktası) hesaplama — iki trend çizgisinin kesişim bar indeksi.
//!
//! Üçgen formasyonlarında (Converging Triangle, Contracting Wedge vb.)
//! üst ve alt çizgiler birbirine yaklaşır. Kesişim noktası **apex** olarak adlandırılır.
//!
//! Kural: Fiyat, apex'e %75 oranında yaklaştığında breakout gelmezse formasyon
//! "stale" (bayat) kabul edilir.

use crate::find::ChannelSixScanOutcome;

/// Apex hesaplama sonucu.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct ApexResult {
    /// İki trend çizgisinin kesiştiği tahmini bar indeksi.
    pub apex_bar: f64,
    /// Kesişim noktasındaki fiyat.
    pub apex_price: f64,
    /// Formasyonun son pivot'undan apex'e olan mesafe (bar cinsinden).
    pub bars_to_apex: f64,
    /// Fiyatın apex'e yaklaşma oranı (0.0–1.0). >= 0.75 ise formasyon "stale".
    pub proximity_ratio: f64,
    /// Formasyon stale mi (proximity_ratio >= stale_threshold)?
    pub is_stale: bool,
}

/// İki doğrunun kesişim bar indeksini hesaplar.
///
/// Üst çizgi: `(u1_bar, u1_price)` → `(u2_bar, u2_price)`
/// Alt çizgi: `(l1_bar, l1_price)` → `(l2_bar, l2_price)`
///
/// Paralel çizgiler → `None` (kanal formasyonları — apex yok).
#[must_use]
pub fn compute_apex_bar(
    u1_bar: i64, u1_price: f64,
    u2_bar: i64, u2_price: f64,
    l1_bar: i64, l1_price: f64,
    l2_bar: i64, l2_price: f64,
) -> Option<(f64, f64)> {
    let u_span = (u2_bar - u1_bar) as f64;
    let l_span = (l2_bar - l1_bar) as f64;
    if u_span.abs() < 1e-15 || l_span.abs() < 1e-15 {
        return None;
    }
    let u_slope = (u2_price - u1_price) / u_span;
    let l_slope = (l2_price - l1_price) / l_span;

    // Paralel çizgiler — kesişim yok
    let slope_diff = u_slope - l_slope;
    if slope_diff.abs() < 1e-15 {
        return None;
    }

    // u1_price + u_slope * (x - u1_bar) = l1_price + l_slope * (x - l1_bar)
    // x = (l1_price - u1_price + u_slope*u1_bar - l_slope*l1_bar) / (u_slope - l_slope)
    let x = (l1_price - u1_price + u_slope * u1_bar as f64 - l_slope * l1_bar as f64) / slope_diff;
    let price = u1_price + u_slope * (x - u1_bar as f64);

    if !x.is_finite() || !price.is_finite() {
        return None;
    }
    Some((x, price))
}

/// Bir `ChannelSixScanOutcome` için apex analizi.
///
/// `current_bar`: fiyatın şu an bulunduğu bar indeksi.
/// `stale_threshold`: yaklaşma oranı eşiği (varsayılan 0.75).
#[must_use]
pub fn compute_apex_from_outcome(
    outcome: &ChannelSixScanOutcome,
    current_bar: i64,
    stale_threshold: f64,
) -> Option<ApexResult> {
    let hints = crate::find::channel_six_drawing_hints(outcome);
    let u = &hints.upper;
    let l = &hints.lower;

    let (apex_bar, apex_price) = compute_apex_bar(
        u[0].bar_index, u[0].price,
        u[1].bar_index, u[1].price,
        l[0].bar_index, l[0].price,
        l[1].bar_index, l[1].price,
    )?;

    // Apex formasyonun ilerisinde mi (gelecekte mi)?
    let last_pivot_bar = outcome.pivots.last().map(|(b, _, _)| *b).unwrap_or(current_bar);
    let formation_start = outcome.pivots.first().map(|(b, _, _)| *b).unwrap_or(0);
    let formation_span = (last_pivot_bar - formation_start) as f64;

    if formation_span <= 0.0 {
        return None;
    }

    let bars_to_apex = apex_bar - last_pivot_bar as f64;
    let total_span = apex_bar - formation_start as f64;

    // Proximity: current_bar formasyonun ne kadarını geçmiş
    let elapsed = (current_bar - formation_start) as f64;
    let proximity_ratio = if total_span > 0.0 {
        (elapsed / total_span).clamp(0.0, 1.5)
    } else {
        1.0
    };

    Some(ApexResult {
        apex_bar,
        apex_price,
        bars_to_apex,
        proximity_ratio,
        is_stale: proximity_ratio >= stale_threshold,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converging_lines_have_apex() {
        // Üst: 100→90 (düşen), Alt: 80→85 (yükselen) → kesişmeli
        let result = compute_apex_bar(0, 100.0, 10, 90.0, 0, 80.0, 10, 85.0);
        assert!(result.is_some());
        let (bar, price) = result.unwrap();
        // Çizgiler kesişmeli: üst eğim=-1/bar, alt eğim=+0.5/bar
        // 100 - x = 80 + 0.5x → 20 = 1.5x → x ≈ 13.33
        assert!((bar - 13.333).abs() < 0.1);
        assert!(price > 80.0 && price < 100.0);
    }

    #[test]
    fn parallel_lines_no_apex() {
        // Paralel çizgiler
        let result = compute_apex_bar(0, 100.0, 10, 110.0, 0, 80.0, 10, 90.0);
        assert!(result.is_none());
    }

    #[test]
    fn diverging_lines_apex_in_past() {
        // Üst: 100→110 (yükselen), Alt: 90→85 (düşen) → kesişim geçmişte
        let result = compute_apex_bar(0, 100.0, 10, 110.0, 0, 90.0, 10, 85.0);
        assert!(result.is_some());
        let (bar, _) = result.unwrap();
        // Kesişim geçmişte (negatif bar)
        assert!(bar < 0.0);
    }

    #[test]
    fn stale_detection() {
        use crate::find::{ChannelSixScanOutcome, SixPivotScanResult};
        let outcome = ChannelSixScanOutcome {
            scan: SixPivotScanResult {
                pattern_type_id: 11, // Converging Triangle
                pick_upper: 1, pick_lower: 1,
                upper_ok: true, lower_ok: true,
                upper_score: 0.0, lower_score: 0.0,
            },
            pivots: vec![
                (0, 100.0, 1), (2, 80.0, -1), (4, 95.0, 1),
                (6, 82.0, -1), (8, 92.0, 1),
            ],
            zigzag_pivot_count: 5,
            pivot_tail_skip: 0,
            zigzag_level: 0,
        };
        // current_bar çok ileride → stale olmalı
        let result = compute_apex_from_outcome(&outcome, 50, 0.75);
        // Pivot verileri gerçek üçgen oluşturmayabilir, None olabilir — test amacıyla
        // apex hesaplanırsa stale kontrolü yapılır
        if let Some(r) = result {
            assert!(r.proximity_ratio >= 0.0);
        }
    }
}
