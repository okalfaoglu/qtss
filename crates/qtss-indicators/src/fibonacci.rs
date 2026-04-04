//! Fibonacci retracement and extension levels from swing pivots.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FibLevel {
    pub ratio: f64,
    pub price: f64,
    pub label: &'static str,
}

const RET_RATIOS: &[(f64, &str)] = &[
    (0.0, "0%"),
    (0.236, "23.6%"),
    (0.382, "38.2%"),
    (0.5, "50%"),
    (0.618, "61.8%"),
    (0.786, "78.6%"),
    (1.0, "100%"),
];

const EXT_RATIOS: &[(f64, &str)] = &[
    (1.0, "100%"),
    (1.272, "127.2%"),
    (1.618, "161.8%"),
    (2.0, "200%"),
    (2.618, "261.8%"),
];

/// Swing high → swing low (veya tersi) arasında Fibonacci retracement seviyeleri.
#[must_use]
pub fn fib_retracements(swing_high: f64, swing_low: f64) -> Vec<FibLevel> {
    let range = swing_high - swing_low;
    RET_RATIOS
        .iter()
        .map(|&(r, lbl)| FibLevel {
            ratio: r,
            price: swing_high - r * range,
            label: lbl,
        })
        .collect()
}

/// Fibonacci extension seviyeleri (AB-CD pattern, trend devamı).
/// `a` → `b` hareketi, `c` retracement noktası.
#[must_use]
pub fn fib_extensions(a: f64, b: f64, c: f64) -> Vec<FibLevel> {
    let range = (b - a).abs();
    let direction = if b > a { 1.0 } else { -1.0 };
    EXT_RATIOS
        .iter()
        .map(|&(r, lbl)| FibLevel {
            ratio: r,
            price: c + direction * r * range,
            label: lbl,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fib_ret_basic() {
        let levels = fib_retracements(100.0, 50.0);
        assert_eq!(levels[0].price, 100.0); // 0%
        assert!((levels[3].price - 75.0).abs() < 1e-9); // 50%
        assert_eq!(levels[6].price, 50.0); // 100%
    }

    #[test]
    fn fib_ext_basic() {
        let levels = fib_extensions(50.0, 100.0, 75.0);
        // 100% ext: 75 + 1.0*50 = 125
        assert!((levels[0].price - 125.0).abs() < 1e-9);
    }
}
