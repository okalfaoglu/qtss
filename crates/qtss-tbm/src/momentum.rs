//! Momentum Pillar — MACD, Stochastic, EMA crossover, divergence analizi.

use crate::pillar::{PillarKind, PillarScore};
use qtss_indicators::divergence::{detect_divergences, DivergenceType};

/// Momentum pillar skoru hesaplar.
///
/// Girdiler (son bar değerleri):
/// - `stoch_k`, `stoch_d`: Stochastic %K, %D (0–100)
/// - `macd_hist`: MACD histogram (pozitif = bullish momentum)
/// - `macd_hist_prev`: Önceki bar MACD histogram
/// - `ema_fast`, `ema_slow`: Kısa/uzun EMA (crossover tespiti)
/// - `price_high_pivots`, `price_low_pivots`: Fiyat pivot (idx, val) çiftleri
/// - `indicator_high_pivots`, `indicator_low_pivots`: İndikatör pivot çiftleri (RSI/MACD)
/// - `is_bottom_search`: true = dip aranıyor (bullish), false = tepe (bearish)
#[must_use]
pub fn score_momentum(
    stoch_k: f64,
    stoch_d: f64,
    macd_hist: f64,
    macd_hist_prev: f64,
    ema_fast: f64,
    ema_slow: f64,
    price_high_pivots: &[(usize, f64)],
    price_low_pivots: &[(usize, f64)],
    indicator_high_pivots: &[(usize, f64)],
    indicator_low_pivots: &[(usize, f64)],
    is_bottom_search: bool,
) -> PillarScore {
    let mut score = 0.0_f64;
    let mut details = Vec::new();

    // 1) Stochastic oversold/overbought (max 25 puan)
    if is_bottom_search {
        if stoch_k < 20.0 {
            let pts = 25.0 * (20.0 - stoch_k) / 20.0;
            score += pts;
            details.push(format!("Stoch oversold K={stoch_k:.1} (+{pts:.1})"));
        }
        // %K > %D crossover in oversold zone
        if stoch_k < 30.0 && stoch_k > stoch_d {
            score += 10.0;
            details.push("Stoch bullish cross in oversold".into());
        }
    } else {
        if stoch_k > 80.0 {
            let pts = 25.0 * (stoch_k - 80.0) / 20.0;
            score += pts;
            details.push(format!("Stoch overbought K={stoch_k:.1} (+{pts:.1})"));
        }
        if stoch_k > 70.0 && stoch_k < stoch_d {
            score += 10.0;
            details.push("Stoch bearish cross in overbought".into());
        }
    }

    // 2) MACD histogram momentum shift (max 20 puan)
    if is_bottom_search {
        if macd_hist > macd_hist_prev && macd_hist_prev < 0.0 {
            score += 20.0;
            details.push("MACD hist turning up from negative".into());
        } else if macd_hist > 0.0 && macd_hist_prev <= 0.0 {
            score += 15.0;
            details.push("MACD hist crossed zero up".into());
        }
    } else {
        if macd_hist < macd_hist_prev && macd_hist_prev > 0.0 {
            score += 20.0;
            details.push("MACD hist turning down from positive".into());
        } else if macd_hist < 0.0 && macd_hist_prev >= 0.0 {
            score += 15.0;
            details.push("MACD hist crossed zero down".into());
        }
    }

    // 3) EMA crossover (max 15 puan)
    if is_bottom_search && ema_fast > ema_slow {
        score += 15.0;
        details.push("EMA fast > slow (bullish cross)".into());
    } else if !is_bottom_search && ema_fast < ema_slow {
        score += 15.0;
        details.push("EMA fast < slow (bearish cross)".into());
    }

    // 4) Divergence (max 30 puan)
    if is_bottom_search {
        let divs = detect_divergences(price_low_pivots, indicator_low_pivots, false);
        let regular_count = divs.iter().filter(|d| d.div_type == DivergenceType::BullishRegular).count();
        let hidden_count = divs.iter().filter(|d| d.div_type == DivergenceType::BullishHidden).count();
        if regular_count > 0 {
            let pts = (regular_count as f64 * 15.0).min(30.0);
            score += pts;
            details.push(format!("{regular_count}x bullish regular divergence (+{pts:.0})"));
        }
        if hidden_count > 0 {
            let pts = (hidden_count as f64 * 10.0).min(20.0);
            score += pts;
            details.push(format!("{hidden_count}x bullish hidden divergence (+{pts:.0})"));
        }
    } else {
        let divs = detect_divergences(price_high_pivots, indicator_high_pivots, true);
        let regular_count = divs.iter().filter(|d| d.div_type == DivergenceType::BearishRegular).count();
        let hidden_count = divs.iter().filter(|d| d.div_type == DivergenceType::BearishHidden).count();
        if regular_count > 0 {
            let pts = (regular_count as f64 * 15.0).min(30.0);
            score += pts;
            details.push(format!("{regular_count}x bearish regular divergence (+{pts:.0})"));
        }
        if hidden_count > 0 {
            let pts = (hidden_count as f64 * 10.0).min(20.0);
            score += pts;
            details.push(format!("{hidden_count}x bearish hidden divergence (+{pts:.0})"));
        }
    }

    PillarScore {
        kind: PillarKind::Momentum,
        score: score.min(100.0),
        weight: 0.30,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_oversold_with_divergence() {
        let price_lows = vec![(10, 100.0), (20, 90.0)]; // lower low
        let ind_lows = vec![(10, 20.0), (20, 30.0)];     // higher low → bullish regular
        let s = score_momentum(
            15.0, 18.0,   // stoch oversold
            -0.5, -1.0,   // hist turning up
            50.0, 51.0,   // no EMA cross
            &[], &price_lows, &[], &ind_lows,
            true,
        );
        assert!(s.score > 40.0);
        assert_eq!(s.kind, PillarKind::Momentum);
    }

    #[test]
    fn top_overbought() {
        let s = score_momentum(
            90.0, 85.0,   // overbought, no bearish cross yet (K>D)
            0.3, 0.5,     // hist turning down
            55.0, 50.0,   // fast > slow (no bearish cross)
            &[], &[], &[], &[],
            false,
        );
        assert!(s.score > 20.0);
    }
}
