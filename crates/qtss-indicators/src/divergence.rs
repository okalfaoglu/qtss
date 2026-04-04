//! Generic divergence detector — pivot karşılaştırması ile indikatör divergence'ı bulur.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DivergenceType {
    /// Price higher high, indicator lower high → bearish
    BearishRegular,
    /// Price lower low, indicator higher low → bullish
    BullishRegular,
    /// Price lower high, indicator higher high → bearish continuation
    BearishHidden,
    /// Price higher low, indicator lower low → bullish continuation
    BullishHidden,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Divergence {
    pub div_type: DivergenceType,
    /// İlk pivot bar indeksi
    pub idx_a: usize,
    /// İkinci pivot bar indeksi
    pub idx_b: usize,
    pub price_a: f64,
    pub price_b: f64,
    pub indicator_a: f64,
    pub indicator_b: f64,
}

/// Pivot çiftlerinden divergence tespit eder.
/// `price_pivots` ve `indicator_pivots`: (bar_index, value) çiftleri.
/// `is_high`: true ise high pivotlar (bearish divergence aranır), false ise low pivotlar (bullish).
#[must_use]
pub fn detect_divergences(
    price_pivots: &[(usize, f64)],
    indicator_pivots: &[(usize, f64)],
    is_high: bool,
) -> Vec<Divergence> {
    let mut results = Vec::new();
    if price_pivots.len() < 2 || indicator_pivots.len() < 2 {
        return results;
    }

    // Her ardışık price pivot çiftini kontrol et, en yakın indicator pivot'u bul
    for w in price_pivots.windows(2) {
        let (idx_a, pa) = w[0];
        let (idx_b, pb) = w[1];

        // idx_a ve idx_b'ye en yakın indicator pivotları bul
        let ia = find_nearest(indicator_pivots, idx_a);
        let ib = find_nearest(indicator_pivots, idx_b);
        if ia.is_none() || ib.is_none() {
            continue;
        }
        let (_, ind_a) = ia.unwrap();
        let (_, ind_b) = ib.unwrap();

        if is_high {
            // Bearish Regular: price HH, indicator LH
            if pb > pa && ind_b < ind_a {
                results.push(Divergence {
                    div_type: DivergenceType::BearishRegular,
                    idx_a, idx_b, price_a: pa, price_b: pb, indicator_a: ind_a, indicator_b: ind_b,
                });
            }
            // Bearish Hidden: price LH, indicator HH
            if pb < pa && ind_b > ind_a {
                results.push(Divergence {
                    div_type: DivergenceType::BearishHidden,
                    idx_a, idx_b, price_a: pa, price_b: pb, indicator_a: ind_a, indicator_b: ind_b,
                });
            }
        } else {
            // Bullish Regular: price LL, indicator HL
            if pb < pa && ind_b > ind_a {
                results.push(Divergence {
                    div_type: DivergenceType::BullishRegular,
                    idx_a, idx_b, price_a: pa, price_b: pb, indicator_a: ind_a, indicator_b: ind_b,
                });
            }
            // Bullish Hidden: price HL, indicator LL
            if pb > pa && ind_b < ind_a {
                results.push(Divergence {
                    div_type: DivergenceType::BullishHidden,
                    idx_a, idx_b, price_a: pa, price_b: pb, indicator_a: ind_a, indicator_b: ind_b,
                });
            }
        }
    }
    results
}

fn find_nearest(pivots: &[(usize, f64)], target_idx: usize) -> Option<(usize, f64)> {
    pivots.iter().min_by_key(|(idx, _)| (*idx as isize - target_idx as isize).unsigned_abs()).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bullish_regular() {
        let price = vec![(10, 100.0), (20, 90.0)]; // LL
        let ind = vec![(10, 30.0), (20, 40.0)];     // HL
        let d = detect_divergences(&price, &ind, false);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].div_type, DivergenceType::BullishRegular);
    }

    #[test]
    fn bearish_regular() {
        let price = vec![(10, 100.0), (20, 110.0)]; // HH
        let ind = vec![(10, 80.0), (20, 70.0)];      // LH
        let d = detect_divergences(&price, &ind, true);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].div_type, DivergenceType::BearishRegular);
    }
}
