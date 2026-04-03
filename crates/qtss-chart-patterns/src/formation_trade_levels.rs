//! Literature-style **entry / stop-loss / take-profit** hints for ACP channel-six outcomes (pattern types 1–13).
//!
//! These are **heuristics** (opposite boundary, measured move, Fib-style extension), not exchange orders.
//! See module-level comments on each branch for the assumed bias per `pattern_type_id`.

use crate::find::{ChannelSixDrawingHints, ChannelSixScanOutcome};
use crate::line_price_at_bar_index;

/// Long vs short plan (range patterns pick one side from price vs mid-channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormationTradeSide {
    Long,
    Short,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FormationTakeProfit {
    pub id: String,
    pub price: f64,
    pub note: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FormationTradeLevels {
    pub pattern_type_id: i32,
    pub side: FormationTradeSide,
    pub entry: f64,
    pub stop_loss: f64,
    pub take_profits: Vec<FormationTakeProfit>,
    pub band_upper_at_bar: f64,
    pub band_lower_at_bar: f64,
    pub reference_bar: i64,
    /// Stable identifier for the rule set (for clients / logging).
    pub method: String,
}

const METHOD_ID: &str = "acp_literature_v1_channel_bands_measured_move";

fn band_high_low(h: &ChannelSixDrawingHints, bar: i64) -> Option<(f64, f64)> {
    let u0 = &h.upper[0];
    let u1 = &h.upper[1];
    let l0 = &h.lower[0];
    let l1 = &h.lower[1];
    let up = line_price_at_bar_index(u0.bar_index, u0.price, u1.bar_index, u1.price, bar)?;
    let lo = line_price_at_bar_index(l0.bar_index, l0.price, l1.bar_index, l1.price, bar)?;
    Some((up.max(lo), up.min(lo)))
}

/// Primary bias per Trendoscope `pattern_type_id` (1–13).
fn trade_side_for_pattern(id: i32, close: f64, band_hi: f64, band_lo: f64) -> FormationTradeSide {
    let mid = (band_hi + band_lo) / 2.0;
    match id {
        // Ascending channel, falling wedge (bullish), ascending triangles → long bias
        1 | 5 | 7 | 10 | 13 => FormationTradeSide::Long,
        // Descending channel, rising wedge (bearish), descending triangles → short bias
        2 | 4 | 8 | 9 | 12 => FormationTradeSide::Short,
        // Range / symmetric expansion / apex congestion → fade from mid
        3 | 6 | 11 => {
            if close >= mid {
                FormationTradeSide::Short
            } else {
                FormationTradeSide::Long
            }
        }
        _ => {
            if close >= mid {
                FormationTradeSide::Short
            } else {
                FormationTradeSide::Long
            }
        }
    }
}

fn sl_buffer(width: f64, entry: f64) -> f64 {
    let w = width.abs();
    (w * 0.025_f64).max(entry.abs() * 1e-8).max(1e-12)
}

/// Build SL / entry / TP ladder at `reference_bar` using `close` as entry anchor.
#[must_use]
pub fn compute_formation_trade_levels(
    outcome: &ChannelSixScanOutcome,
    reference_bar: i64,
    close: f64,
) -> Option<FormationTradeLevels> {
    let id = outcome.scan.pattern_type_id;
    if !(1..=13).contains(&id) {
        return None;
    }

    let hints = crate::find::channel_six_drawing_hints(outcome);
    let (band_hi, band_lo) = band_high_low(&hints, reference_bar)?;
    let width = band_hi - band_lo;
    if !width.is_finite() || width <= 1e-15 {
        return None;
    }

    let side = trade_side_for_pattern(id, close, band_hi, band_lo);
    let entry = close;
    let buf = sl_buffer(width, entry);

    let (stop_loss, take_profits) = match side {
        FormationTradeSide::Long => {
            let sl = band_lo - buf;
            if sl >= entry {
                return None;
            }
            let mut tps = Vec::new();
            tps.push(FormationTakeProfit {
                id: "tp1_opposite_band".to_string(),
                price: band_hi,
                note: "Opposite channel boundary (mean-reversion / first objective).".to_string(),
            });
            let tp2 = band_hi + width;
            if tp2 > entry && tp2.is_finite() {
                tps.push(FormationTakeProfit {
                    id: "tp2_measured_move".to_string(),
                    price: tp2,
                    note: "Measured move: +1× channel width beyond upper band.".to_string(),
                });
            }
            let tp3 = band_hi + 1.618_f64 * width;
            if tp3 > entry && tp3 > band_hi && tp3.is_finite() {
                tps.push(FormationTakeProfit {
                    id: "tp3_extension_1618".to_string(),
                    price: tp3,
                    note: "Fib-style extension ~1.618× width beyond upper band.".to_string(),
                });
            }
            (sl, tps)
        }
        FormationTradeSide::Short => {
            let sl = band_hi + buf;
            if sl <= entry {
                return None;
            }
            let mut tps = Vec::new();
            tps.push(FormationTakeProfit {
                id: "tp1_opposite_band".to_string(),
                price: band_lo,
                note: "Opposite channel boundary (mean-reversion / first objective).".to_string(),
            });
            let tp2 = band_lo - width;
            if tp2 < entry && tp2.is_finite() {
                tps.push(FormationTakeProfit {
                    id: "tp2_measured_move".to_string(),
                    price: tp2,
                    note: "Measured move: −1× channel width beyond lower band.".to_string(),
                });
            }
            let tp3 = band_lo - 1.618_f64 * width;
            if tp3 < entry && tp3 < band_lo && tp3.is_finite() {
                tps.push(FormationTakeProfit {
                    id: "tp3_extension_1618".to_string(),
                    price: tp3,
                    note: "Fib-style extension ~1.618× width beyond lower band.".to_string(),
                });
            }
            (sl, tps)
        }
    };

    Some(FormationTradeLevels {
        pattern_type_id: id,
        side,
        entry,
        stop_loss,
        take_profits,
        band_upper_at_bar: band_hi,
        band_lower_at_bar: band_lo,
        reference_bar,
        method: METHOD_ID.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::find::{ChannelSixScanOutcome, SixPivotScanResult};

    fn sample_outcome(id: i32) -> ChannelSixScanOutcome {
        ChannelSixScanOutcome {
            scan: SixPivotScanResult {
                pattern_type_id: id,
                pick_upper: 1,
                pick_lower: 1,
                upper_ok: true,
                lower_ok: true,
                upper_score: 0.0,
                lower_score: 0.0,
            },
            pivots: vec![
                (0_i64, 100.0, 1),
                (2, 95.0, -1),
                (4, 102.0, 1),
                (6, 96.0, -1),
                (8, 104.0, 1),
            ],
            zigzag_pivot_count: 5,
            pivot_tail_skip: 0,
            zigzag_level: 0,
        }
    }

    #[test]
    fn ascending_channel_long_has_sl_below_lower() {
        let o = sample_outcome(1);
        let lv = compute_formation_trade_levels(&o, 8, 100.0).expect("levels");
        assert_eq!(lv.side, FormationTradeSide::Long);
        assert!(lv.stop_loss < lv.entry);
        assert!(lv.band_upper_at_bar > lv.band_lower_at_bar);
        assert!(!lv.take_profits.is_empty());
    }

    #[test]
    fn descending_channel_short_has_sl_above_upper() {
        let o = sample_outcome(2);
        let lv = compute_formation_trade_levels(&o, 8, 100.0).expect("levels");
        assert_eq!(lv.side, FormationTradeSide::Short);
        assert!(lv.stop_loss > lv.entry);
    }
}
