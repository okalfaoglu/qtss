//! Order Block (OB) detector.
//!
//! An Order Block is the last opposing candle before a strong impulsive move.
//! Institutional orders cluster at these levels — price tends to return to
//! the OB zone before continuing.
//!
//! - **Bullish OB**: Last bearish candle before a strong bullish impulse.
//!   Zone = [candle.low, candle.high]. Expect price to bounce from this zone.
//! - **Bearish OB**: Last bullish candle before a strong bearish impulse.
//!   Zone = [candle.low, candle.high]. Expect price to reject from this zone.

use serde::{Deserialize, Serialize};

use crate::OhlcBar;
use crate::helpers::avg_volume;

// ── Config ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBlockConfig {
    /// Minimum impulse size as ATR multiple to qualify the move after the OB.
    #[serde(default = "default_impulse_atr_mult")]
    pub impulse_atr_mult: f64,
    /// Number of impulse candles to measure (must move >= impulse_atr_mult * ATR).
    #[serde(default = "default_impulse_candles")]
    pub impulse_candles: usize,
    /// How many bars back to scan for OBs.
    #[serde(default = "default_scan_lookback")]
    pub scan_lookback: usize,
    /// Only report unmitigated (price hasn't returned to OB zone) OBs.
    #[serde(default = "default_unmitigated_only")]
    pub unmitigated_only: bool,
    /// Volume spike multiplier for the impulse candles (quality bonus).
    #[serde(default = "default_volume_spike_mult")]
    pub volume_spike_mult: f64,
    /// Volume lookback for averaging.
    #[serde(default = "default_volume_lookback")]
    pub volume_lookback: usize,
}

fn default_impulse_atr_mult() -> f64 { 1.5 }
fn default_impulse_candles() -> usize { 3 }
fn default_scan_lookback() -> usize { 50 }
fn default_unmitigated_only() -> bool { true }
fn default_volume_spike_mult() -> f64 { 1.3 }
fn default_volume_lookback() -> usize { 20 }

impl Default for OrderBlockConfig {
    fn default() -> Self {
        Self {
            impulse_atr_mult: default_impulse_atr_mult(),
            impulse_candles: default_impulse_candles(),
            scan_lookback: default_scan_lookback(),
            unmitigated_only: default_unmitigated_only(),
            volume_spike_mult: default_volume_spike_mult(),
            volume_lookback: default_volume_lookback(),
        }
    }
}

// ── Output ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBlockMatch {
    /// "bullish_ob" or "bearish_ob"
    pub subkind: String,
    /// Bar index of the order block candle.
    pub bar_index: i64,
    /// Upper edge of the OB zone.
    pub ob_high: f64,
    /// Lower edge of the OB zone.
    pub ob_low: f64,
    /// Size of the impulse move after the OB (in price).
    pub impulse_size: f64,
    /// Impulse / ATR ratio.
    pub impulse_atr_ratio: f64,
    /// Whether price has returned to and through the OB zone (mitigated).
    pub mitigated: bool,
    /// Quality score 0.0–1.0.
    pub score: f64,
    /// Whether the impulse had above-average volume.
    pub volume_confirmed: bool,
}

// ── Detection ───────────────────────────────────────────────────────

pub fn detect_order_blocks(
    bars: &[OhlcBar],
    atr_value: f64,
    cfg: &OrderBlockConfig,
) -> Vec<OrderBlockMatch> {
    let n = bars.len();
    if n < cfg.impulse_candles + 3 {
        return Vec::new();
    }

    let scan_start = n.saturating_sub(cfg.scan_lookback).max(1);
    let min_impulse = atr_value * cfg.impulse_atr_mult;
    let mut results = Vec::new();

    for i in scan_start..(n.saturating_sub(cfg.impulse_candles)) {
        let ob = &bars[i];
        let is_bearish_candle = ob.close < ob.open;
        let is_bullish_candle = ob.close > ob.open;

        if !is_bearish_candle && !is_bullish_candle {
            continue;
        }

        // Measure impulse after the OB candle
        let impulse_end = (i + cfg.impulse_candles).min(n - 1);
        let impulse_bars = &bars[i + 1..=impulse_end];

        if is_bearish_candle {
            // Potential bullish OB: need strong upward impulse after
            let max_high = impulse_bars.iter().map(|b| b.high).fold(f64::NEG_INFINITY, f64::max);
            let impulse_size = max_high - ob.low;
            if impulse_size < min_impulse {
                continue;
            }

            // Verify all impulse candles are mostly bullish
            let bull_count = impulse_bars.iter().filter(|b| b.close > b.open).count();
            if bull_count < (cfg.impulse_candles + 1) / 2 {
                continue;
            }

            let mitigated = check_mitigation(bars, impulse_end + 1, ob.low, "bullish_ob");

            if cfg.unmitigated_only && mitigated {
                continue;
            }

            let vol_avg = avg_volume(bars, i, cfg.volume_lookback);
            let impulse_vol: f64 = impulse_bars.iter()
                .filter_map(|b| b.volume)
                .sum::<f64>() / cfg.impulse_candles as f64;
            let volume_confirmed = match vol_avg {
                Some(avg) if avg > 1e-12 => impulse_vol >= avg * cfg.volume_spike_mult,
                _ => false,
            };

            let impulse_atr_ratio = if atr_value > 1e-12 { impulse_size / atr_value } else { 0.0 };
            let score = compute_ob_score(impulse_atr_ratio, volume_confirmed, mitigated);

            results.push(OrderBlockMatch {
                subkind: "bullish_ob".into(),
                bar_index: ob.bar_index,
                ob_high: ob.high,
                ob_low: ob.low,
                impulse_size,
                impulse_atr_ratio,
                mitigated,
                score,
                volume_confirmed,
            });
        } else {
            // Potential bearish OB: need strong downward impulse after
            let min_low = impulse_bars.iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
            let impulse_size = ob.high - min_low;
            if impulse_size < min_impulse {
                continue;
            }

            let bear_count = impulse_bars.iter().filter(|b| b.close < b.open).count();
            if bear_count < (cfg.impulse_candles + 1) / 2 {
                continue;
            }

            let mitigated = check_mitigation(bars, impulse_end + 1, ob.high, "bearish_ob");

            if cfg.unmitigated_only && mitigated {
                continue;
            }

            let vol_avg = avg_volume(bars, i, cfg.volume_lookback);
            let impulse_vol: f64 = impulse_bars.iter()
                .filter_map(|b| b.volume)
                .sum::<f64>() / cfg.impulse_candles as f64;
            let volume_confirmed = match vol_avg {
                Some(avg) if avg > 1e-12 => impulse_vol >= avg * cfg.volume_spike_mult,
                _ => false,
            };

            let impulse_atr_ratio = if atr_value > 1e-12 { impulse_size / atr_value } else { 0.0 };
            let score = compute_ob_score(impulse_atr_ratio, volume_confirmed, mitigated);

            results.push(OrderBlockMatch {
                subkind: "bearish_ob".into(),
                bar_index: ob.bar_index,
                ob_high: ob.high,
                ob_low: ob.low,
                impulse_size,
                impulse_atr_ratio,
                mitigated,
                score,
                volume_confirmed,
            });
        }
    }

    results
}

fn check_mitigation(bars: &[OhlcBar], from: usize, level: f64, subkind: &str) -> bool {
    for b in bars.iter().skip(from) {
        match subkind {
            "bullish_ob" => {
                if b.low <= level {
                    return true;
                }
            }
            "bearish_ob" => {
                if b.high >= level {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn compute_ob_score(impulse_atr_ratio: f64, volume_confirmed: bool, mitigated: bool) -> f64 {
    let impulse_score = (impulse_atr_ratio / 4.0).clamp(0.0, 0.5);
    let vol_score = if volume_confirmed { 0.25 } else { 0.05 };
    let mit_score = if !mitigated { 0.25 } else { 0.05 };
    (impulse_score + vol_score + mit_score).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(i: i64, o: f64, h: f64, l: f64, c: f64) -> OhlcBar {
        OhlcBar { open: o, high: h, low: l, close: c, bar_index: i, volume: Some(1000.0) }
    }

    #[test]
    fn detects_bullish_ob() {
        let bars = vec![
            bar(0, 100.0, 101.0, 99.0, 100.0),
            bar(1, 100.0, 100.5, 98.0, 98.5),  // bearish candle = OB candidate
            bar(2, 99.0, 103.0, 98.5, 102.5),   // impulse up
            bar(3, 102.5, 106.0, 102.0, 105.0),  // impulse continues
            bar(4, 105.0, 107.0, 104.0, 106.5),  // impulse continues
            bar(5, 106.0, 108.0, 105.5, 107.0),  // no fill
        ];
        let obs = detect_order_blocks(&bars, 2.0, &OrderBlockConfig {
            impulse_atr_mult: 1.0,
            unmitigated_only: false,
            scan_lookback: 50,
            ..Default::default()
        });
        assert!(!obs.is_empty());
        assert_eq!(obs[0].subkind, "bullish_ob");
    }
}
