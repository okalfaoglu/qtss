//! Structure Pillar — Fibonacci seviyeleri, destek/direnç, Bollinger pozisyonu, chart pattern.

use crate::pillar::{PillarKind, PillarScore};

/// Structure pillar skoru hesaplar.
///
/// - `fib_proximity`: Fiyatın en yakın Fibonacci seviyesine yakınlığı (0.0–1.0, 1.0 = tam üzerinde)
/// - `fib_level_name`: Seviye adı ("61.8%", "78.6%" vb.)
/// - `bb_percent_b`: Bollinger %B (0 = alt band, 1 = üst band)
/// - `bb_squeeze`: Bollinger squeeze aktif mi
/// - `atr_compression`: ATR sıkışma aktif mi
/// - `formation_quality`: En güçlü formasyon quality skoru (0.0–1.0), yoksa 0
/// - `formation_name`: Formasyon adı
/// - `is_bottom_search`: true = dip, false = tepe
#[must_use]
pub fn score_structure(
    fib_proximity: f64,
    fib_level_name: &str,
    bb_percent_b: f64,
    bb_squeeze: bool,
    atr_compression: bool,
    formation_quality: f64,
    formation_name: &str,
    is_bottom_search: bool,
) -> PillarScore {
    let mut score = 0.0_f64;
    let mut details = Vec::new();

    // 1) Fibonacci seviyesine yakınlık (max 30 puan)
    if fib_proximity > 0.8 {
        let pts = 30.0 * fib_proximity;
        score += pts;
        details.push(format!("Near Fib {fib_level_name} (proximity {fib_proximity:.2}, +{pts:.1})"));
    } else if fib_proximity > 0.5 {
        let pts = 15.0 * fib_proximity;
        score += pts;
        details.push(format!("Approaching Fib {fib_level_name} (+{pts:.1})"));
    }

    // 2) Bollinger Band pozisyonu (max 25 puan)
    if is_bottom_search && bb_percent_b < 0.05 {
        score += 25.0;
        details.push(format!("Price at lower BB (%B={bb_percent_b:.2})"));
    } else if is_bottom_search && bb_percent_b < 0.2 {
        score += 15.0;
        details.push(format!("Price near lower BB (%B={bb_percent_b:.2})"));
    } else if !is_bottom_search && bb_percent_b > 0.95 {
        score += 25.0;
        details.push(format!("Price at upper BB (%B={bb_percent_b:.2})"));
    } else if !is_bottom_search && bb_percent_b > 0.8 {
        score += 15.0;
        details.push(format!("Price near upper BB (%B={bb_percent_b:.2})"));
    }

    // 3) Volatilite sıkışma — potansiyel patlama (max 15 puan)
    if bb_squeeze || atr_compression {
        score += 15.0;
        let label = match (bb_squeeze, atr_compression) {
            (true, true) => "BB squeeze + ATR compression",
            (true, false) => "BB squeeze",
            (false, true) => "ATR compression",
            _ => unreachable!(),
        };
        details.push(format!("{label} → breakout imminent (+15)"));
    }

    // 4) Chart pattern/formasyon (max 30 puan)
    if formation_quality > 0.0 {
        let pts = 30.0 * formation_quality;
        score += pts;
        details.push(format!("Formation: {formation_name} quality={formation_quality:.0}% (+{pts:.1})"));
    }

    PillarScore {
        kind: PillarKind::Structure,
        score: score.min(100.0),
        weight: 0.30,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_at_fib_with_squeeze() {
        let s = score_structure(
            0.95, "61.8%",
            0.02,  // at lower BB
            true,  // squeeze
            true,  // ATR compression
            0.85,  // strong formation
            "Double Bottom",
            true,
        );
        assert!(s.score > 70.0);
    }

    #[test]
    fn top_at_upper_bb() {
        let s = score_structure(
            0.3, "38.2%",
            0.98,
            false, false,
            0.0, "",
            false,
        );
        assert!(s.score > 20.0);
    }
}
