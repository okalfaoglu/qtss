//! Volume Profile builders.
//!
//! Trait + implementations dispatch (CLAUDE.md #1). Adding a new source
//! (tick stream, aggregated trades, footprint chart) is one extra
//! `impl VolumeProfileBuilder` — callers register it without editing
//! any central match arm.

use crate::config::VProfileConfig;
use crate::error::{VProfileError, VProfileResult};
use crate::profile::{VPBin, VolumeProfile};
use qtss_domain::v2::bar::Bar;
use rust_decimal::Decimal;

/// Common interface — every source maps to the same `VolumeProfile`.
pub trait VolumeProfileBuilder {
    /// Stable identifier (used in tracing / persistence).
    fn name(&self) -> &'static str;

    /// Build the profile or return an error if the input is too thin /
    /// inconsistent.
    fn build(&self, bars: &[Bar], cfg: &VProfileConfig) -> VProfileResult<VolumeProfile>;
}

/// Bar-based approximation: each bar's `volume` is distributed across
/// the bins it overlaps (proportional to the bin overlap with the bar's
/// `[low, high]` interval). When tick data is unavailable this is the
/// industry-standard fall-back used by TradingView's "Volume Profile
/// Visible Range" indicator.
pub struct BarBasedBuilder;

impl VolumeProfileBuilder for BarBasedBuilder {
    fn name(&self) -> &'static str {
        "bar_based"
    }

    fn build(&self, bars: &[Bar], cfg: &VProfileConfig) -> VProfileResult<VolumeProfile> {
        cfg.validate()?;
        if bars.len() < cfg.min_bars_for_profile {
            return Err(VProfileError::InsufficientInput(format!(
                "need at least {} bars, got {}",
                cfg.min_bars_for_profile,
                bars.len()
            )));
        }
        // Window low/high — across all bars.
        let (mut lo, mut hi) = (bars[0].low, bars[0].high);
        for b in bars.iter().skip(1) {
            if b.low < lo {
                lo = b.low;
            }
            if b.high > hi {
                hi = b.high;
            }
        }
        if hi <= lo {
            return Err(VProfileError::InsufficientInput(
                "degenerate range — high <= low".into(),
            ));
        }
        let bin_count = cfg.bin_count;
        let bin_size = (hi - lo) / Decimal::from(bin_count as i64);
        if bin_size <= Decimal::ZERO {
            return Err(VProfileError::InsufficientInput(
                "bin_size collapsed to zero — range too narrow".into(),
            ));
        }
        // Pre-allocate bins.
        let mut bins: Vec<VPBin> = (0..bin_count)
            .map(|i| {
                let p_low = lo + bin_size * Decimal::from(i as i64);
                let p_high = if i == bin_count - 1 {
                    hi
                } else {
                    lo + bin_size * Decimal::from((i + 1) as i64)
                };
                VPBin {
                    price_low: p_low,
                    price_high: p_high,
                    volume: Decimal::ZERO,
                }
            })
            .collect();

        // Distribute each bar's volume proportionally across the bins
        // it overlaps. Volume per bin = bar.volume × (overlap / bar_range).
        for b in bars {
            if b.volume <= Decimal::ZERO || b.high <= b.low {
                continue;
            }
            let bar_range = b.high - b.low;
            // First and last bin indices the bar touches.
            let first = bin_index(b.low, lo, bin_size, bin_count);
            let last = bin_index(b.high, lo, bin_size, bin_count);
            for i in first..=last {
                let bin = &mut bins[i];
                let ov_lo = if b.low > bin.price_low { b.low } else { bin.price_low };
                let ov_hi = if b.high < bin.price_high { b.high } else { bin.price_high };
                if ov_hi <= ov_lo {
                    continue;
                }
                let overlap = ov_hi - ov_lo;
                let share = (overlap * b.volume) / bar_range;
                bin.volume += share;
            }
        }

        Ok(VolumeProfile::derive(lo, hi, bin_size, bins, cfg))
    }
}

fn bin_index(price: Decimal, lo: Decimal, bin_size: Decimal, bin_count: usize) -> usize {
    if bin_size <= Decimal::ZERO {
        return 0;
    }
    let raw = ((price - lo) / bin_size).to_string().parse::<f64>().unwrap_or(0.0);
    let idx = raw.floor() as i64;
    if idx < 0 {
        0
    } else if idx as usize >= bin_count {
        bin_count - 1
    } else {
        idx as usize
    }
}
