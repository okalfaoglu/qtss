//! Volume Weighted Average Price + standard deviation bands.

#[derive(Debug, Clone)]
pub struct VwapResult {
    pub vwap: Vec<f64>,
    pub upper_1: Vec<f64>,
    pub lower_1: Vec<f64>,
    pub upper_2: Vec<f64>,
    pub lower_2: Vec<f64>,
}

/// VWAP hesaplar (session başından kümülatif).
/// `session_starts[i] = true` ise yeni session başlar (günlük reset).
/// Eğer `session_starts` boşsa tüm seri tek session sayılır.
#[must_use]
pub fn vwap(highs: &[f64], lows: &[f64], closes: &[f64], volumes: &[f64], session_starts: &[bool]) -> VwapResult {
    let n = closes.len();
    let mut vw = vec![f64::NAN; n];
    let mut u1 = vec![f64::NAN; n];
    let mut l1 = vec![f64::NAN; n];
    let mut u2 = vec![f64::NAN; n];
    let mut l2 = vec![f64::NAN; n];
    if n == 0 {
        return VwapResult { vwap: vw, upper_1: u1, lower_1: l1, upper_2: u2, lower_2: l2 };
    }

    let mut cum_tp_vol = 0.0_f64;
    let mut cum_vol = 0.0_f64;
    let mut cum_tp2_vol = 0.0_f64;

    for i in 0..n {
        let new_session = if session_starts.is_empty() { i == 0 } else { session_starts[i] };
        if new_session {
            cum_tp_vol = 0.0;
            cum_vol = 0.0;
            cum_tp2_vol = 0.0;
        }
        let tp = (highs[i] + lows[i] + closes[i]) / 3.0;
        cum_tp_vol += tp * volumes[i];
        cum_vol += volumes[i];
        cum_tp2_vol += tp * tp * volumes[i];

        if cum_vol.abs() < 1e-15 {
            continue;
        }
        let v = cum_tp_vol / cum_vol;
        vw[i] = v;
        let variance = (cum_tp2_vol / cum_vol - v * v).max(0.0);
        let sd = variance.sqrt();
        u1[i] = v + sd;
        l1[i] = v - sd;
        u2[i] = v + 2.0 * sd;
        l2[i] = v - 2.0 * sd;
    }

    VwapResult { vwap: vw, upper_1: u1, lower_1: l1, upper_2: u2, lower_2: l2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vwap_basic() {
        let h = vec![11.0, 12.0, 13.0];
        let l = vec![9.0, 10.0, 11.0];
        let c = vec![10.0, 11.0, 12.0];
        let v = vec![100.0, 100.0, 100.0];
        let r = vwap(&h, &l, &c, &v, &[]);
        assert!(!r.vwap[0].is_nan());
        // Equal volumes, tp = 10, 11, 12 → vwap[2] = (10+11+12)/3 = 11
        assert!((r.vwap[2] - 11.0).abs() < 1e-9);
    }
}
