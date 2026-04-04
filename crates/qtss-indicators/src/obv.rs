//! On-Balance Volume.

/// OBV hesaplar. Closes[i] > closes[i-1] → OBV += vol, < → OBV -= vol.
#[must_use]
pub fn obv(closes: &[f64], volumes: &[f64]) -> Vec<f64> {
    let n = closes.len();
    if n == 0 {
        return vec![];
    }
    let mut out = vec![0.0; n];
    for i in 1..n {
        out[i] = if closes[i] > closes[i - 1] {
            out[i - 1] + volumes[i]
        } else if closes[i] < closes[i - 1] {
            out[i - 1] - volumes[i]
        } else {
            out[i - 1]
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obv_basic() {
        let c = vec![10.0, 11.0, 10.5, 12.0];
        let v = vec![100.0, 200.0, 150.0, 300.0];
        let r = obv(&c, &v);
        assert_eq!(r[0], 0.0);
        assert_eq!(r[1], 200.0);   // up
        assert_eq!(r[2], 50.0);    // down
        assert_eq!(r[3], 350.0);   // up
    }
}
