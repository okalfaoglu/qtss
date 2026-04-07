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

    // 2) MACD histogram momentum shift (max 20 puan — tek koşul, üst üste binmez)
    //
    // **Shadowing:** İlk dal `prev < 0 && hist > prev` iken +20 verir. Negatiften pozitife tek mumda
    // sıçrama (`prev < 0`, `hist > 0`) her zaman `hist > prev` olduğundan yine bu dal tetiklenir;
    // `else if` (sıfırı yukarı kesme +15) bu senaryoda **hiç çalışmaz**. +15 yalnızca tipik olarak
    // `prev == 0` ve `hist > 0` gibi “önceki bar tam sıfır / üst sınır” durumlarında kalır — kasıtlı
    // ayrım değilse birleştirilebilir veya mesaj tekilleştirilir.
    if is_bottom_search {
        if macd_hist_prev < 0.0 && macd_hist > macd_hist_prev {
            score += 20.0;
            let msg = if macd_hist > 0.0 {
                "MACD hist rising from negative (incl. cross above zero)"
            } else {
                "MACD hist turning up from negative"
            };
            details.push(msg.into());
        } else if macd_hist > 0.0 && macd_hist_prev <= 0.0 {
            score += 15.0;
            details.push("MACD hist crossed zero up (from flat/zero)".into());
        }
    } else {
        if macd_hist_prev > 0.0 && macd_hist < macd_hist_prev {
            score += 20.0;
            let msg = if macd_hist < 0.0 {
                "MACD hist falling from positive (incl. cross below zero)"
            } else {
                "MACD hist turning down from positive"
            };
            details.push(msg.into());
        } else if macd_hist < 0.0 && macd_hist_prev >= 0.0 {
            score += 15.0;
            details.push("MACD hist crossed zero down (from flat/zero)".into());
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
        let mut regular_count = 0usize;
        let mut hidden_count = 0usize;
        for d in &divs {
            match d.div_type {
                DivergenceType::BullishRegular => regular_count += 1,
                DivergenceType::BullishHidden => hidden_count += 1,
                _ => {}
            }
        }
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
        let mut regular_count = 0usize;
        let mut hidden_count = 0usize;
        for d in &divs {
            match d.div_type {
                DivergenceType::BearishRegular => regular_count += 1,
                DivergenceType::BearishHidden => hidden_count += 1,
                _ => {}
            }
        }
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

    #[test]
    fn macd_cross_from_negative_uses_first_branch_not_zero_cross_points() {
        let s = score_momentum(
            50.0,
            50.0,
            0.05,
            -0.5,
            0.0,
            0.0,
            &[],
            &[],
            &[],
            &[],
            true,
        );
        let macd_lines: Vec<&str> = s
            .details
            .iter()
            .filter(|d| d.contains("MACD hist"))
            .map(String::as_str)
            .collect();
        assert_eq!(macd_lines.len(), 1);
        assert!(
            macd_lines[0].contains("rising from negative") && macd_lines[0].contains("cross above zero"),
            "expected combined message, got {:?}",
            macd_lines
        );
        // 20 from MACD block only (no stoch/ema/div)
        assert!((s.score - 20.0).abs() < 1e-9);
    }

    #[test]
    fn macd_cross_up_from_exact_zero_hits_second_branch() {
        let s = score_momentum(
            50.0,
            50.0,
            0.1,
            0.0,
            0.0,
            0.0,
            &[],
            &[],
            &[],
            &[],
            true,
        );
        assert!(
            s.details.iter().any(|d| d.contains("crossed zero up")),
            "{:?}",
            s.details
        );
        assert!((s.score - 15.0).abs() < 1e-9);
    }
}
