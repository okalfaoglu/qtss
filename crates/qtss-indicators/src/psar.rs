//! Parabolic SAR — J. Welles Wilder's stop-and-reverse trailing stop.
//!
//! Returns the SAR dot per bar plus a `trend` series (+1 bull / -1 bear).
//! Canonical defaults: acc_start=0.02, acc_step=0.02, acc_max=0.2.

#[derive(Debug, Clone)]
pub struct PsarResult {
    pub sar: Vec<f64>,
    pub trend: Vec<i8>,
}

pub fn psar(
    highs: &[f64],
    lows: &[f64],
    acc_start: f64,
    acc_step: f64,
    acc_max: f64,
) -> PsarResult {
    let n = highs.len().min(lows.len());
    let mut sar = vec![f64::NAN; n];
    let mut trend = vec![0i8; n];
    if n < 2 {
        return PsarResult { sar, trend };
    }
    // Seed: pick initial trend from first 2 bars.
    let mut up = highs[0] < highs[1];
    let mut ep = if up { highs[1] } else { lows[1] }; // extreme point
    let mut af = acc_start;
    sar[0] = if up { lows[0] } else { highs[0] };
    sar[1] = sar[0];
    trend[0] = if up { 1 } else { -1 };
    trend[1] = trend[0];
    for i in 2..n {
        let prev_sar = sar[i - 1];
        let mut s = prev_sar + af * (ep - prev_sar);
        if up {
            // Bull: SAR never above the prior two lows.
            s = s.min(lows[i - 1]).min(lows[i - 2]);
            if highs[i] > ep {
                ep = highs[i];
                af = (af + acc_step).min(acc_max);
            }
            if lows[i] < s {
                // Flip to bear.
                up = false;
                s = ep;
                ep = lows[i];
                af = acc_start;
            }
        } else {
            s = s.max(highs[i - 1]).max(highs[i - 2]);
            if lows[i] < ep {
                ep = lows[i];
                af = (af + acc_step).min(acc_max);
            }
            if highs[i] > s {
                up = true;
                s = ep;
                ep = highs[i];
                af = acc_start;
            }
        }
        sar[i] = s;
        trend[i] = if up { 1 } else { -1 };
    }
    PsarResult { sar, trend }
}
