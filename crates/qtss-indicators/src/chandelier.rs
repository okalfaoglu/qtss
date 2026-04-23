//! Chandelier Exit — Chuck LeBeau's volatility-adjusted trailing stop.
//!
//! Long exit  = highest_high(N) − ATR(N) × mult
//! Short exit = lowest_low(N)  + ATR(N) × mult
//!
//! Default mult 3.0, period 22 (roughly one trading month).

use crate::volatility::atr;

#[derive(Debug, Clone)]
pub struct Chandelier {
    pub long_exit: Vec<f64>,
    pub short_exit: Vec<f64>,
}

pub fn chandelier(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    period: usize,
    mult: f64,
) -> Chandelier {
    let n = closes.len();
    let atr_series = atr(highs, lows, closes, period);
    let mut long_exit = vec![f64::NAN; n];
    let mut short_exit = vec![f64::NAN; n];
    if period == 0 || n < period {
        return Chandelier {
            long_exit,
            short_exit,
        };
    }
    for i in (period - 1)..n {
        if atr_series[i].is_nan() {
            continue;
        }
        let mut hh = f64::NEG_INFINITY;
        let mut ll = f64::INFINITY;
        for j in (i + 1 - period)..=i {
            if highs[j] > hh {
                hh = highs[j];
            }
            if lows[j] < ll {
                ll = lows[j];
            }
        }
        long_exit[i] = hh - mult * atr_series[i];
        short_exit[i] = ll + mult * atr_series[i];
    }
    Chandelier {
        long_exit,
        short_exit,
    }
}
