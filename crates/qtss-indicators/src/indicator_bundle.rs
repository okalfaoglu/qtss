//! Tüm indikatörleri tek seferde hesaplar.

use serde::{Deserialize, Serialize};
use crate::{
    bollinger::{bollinger, BollingerResult},
    cvd::cvd,
    ema::{ema, sma},
    macd::{macd, MacdResult},
    mfi::mfi,
    obv::obv,
    stochastic::{stochastic, StochasticResult},
    volatility::atr,
    vwap::{vwap, VwapResult},
};

/// Tüm indikatör sonuçlarını barındıran bundle.
#[derive(Debug, Clone)]
pub struct IndicatorBundle {
    pub ema_9: Vec<f64>,
    pub ema_21: Vec<f64>,
    pub ema_55: Vec<f64>,
    pub sma_20: Vec<f64>,
    pub sma_50: Vec<f64>,
    pub sma_200: Vec<f64>,
    pub macd: MacdResult,
    pub bollinger: BollingerResult,
    pub stochastic: StochasticResult,
    pub mfi_14: Vec<f64>,
    pub obv: Vec<f64>,
    pub cvd: Vec<f64>,
    pub vwap: VwapResult,
    pub atr_14: Vec<f64>,
}

/// OHLCV bar verisinden tüm indikatörleri tek çağrıda hesaplar.
#[must_use]
pub fn compute_all(
    opens: &[f64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
    session_starts: &[bool],
) -> IndicatorBundle {
    IndicatorBundle {
        ema_9: ema(closes, 9),
        ema_21: ema(closes, 21),
        ema_55: ema(closes, 55),
        sma_20: sma(closes, 20),
        sma_50: sma(closes, 50),
        sma_200: sma(closes, 200),
        macd: macd(closes, 12, 26, 9),
        bollinger: bollinger(closes, 20, 2.0),
        stochastic: stochastic(highs, lows, closes, 14, 3),
        mfi_14: mfi(highs, lows, closes, volumes, 14),
        obv: obv(closes, volumes),
        cvd: cvd(highs, lows, closes, volumes),
        vwap: vwap(highs, lows, closes, volumes, session_starts),
        atr_14: atr(highs, lows, closes, 14),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_smoke() {
        let n = 100;
        let c: Vec<f64> = (1..=n).map(|x| 50.0 + (x as f64 * 0.1).sin() * 10.0).collect();
        let h: Vec<f64> = c.iter().map(|v| v + 1.0).collect();
        let l: Vec<f64> = c.iter().map(|v| v - 1.0).collect();
        let o: Vec<f64> = c.iter().map(|v| v - 0.5).collect();
        let v: Vec<f64> = (0..n).map(|i| 1000.0 + i as f64 * 10.0).collect();
        let b = compute_all(&o, &h, &l, &c, &v, &[]);
        assert_eq!(b.ema_9.len(), n);
        assert_eq!(b.macd.macd_line.len(), n);
        assert_eq!(b.obv.len(), n);
    }
}
