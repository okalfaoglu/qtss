//! `VolumeProfile` data type + post-processing (VAH/VAL/HVN/LVN).
//!
//! The builders fill `bins` and `vpoc`; `VolumeProfile::derive` then
//! computes value area + node lists from the bin distribution.

use crate::config::VProfileConfig;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// One discrete price bin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VPBin {
    /// Inclusive lower price boundary.
    pub price_low: Decimal,
    /// Exclusive upper price boundary (last bin includes the upper).
    pub price_high: Decimal,
    /// Cumulative volume traded inside this bin.
    pub volume: Decimal,
}

impl VPBin {
    pub fn mid(&self) -> Decimal {
        (self.price_low + self.price_high) / Decimal::from(2)
    }
}

/// Composite volume profile across a window of bars/trades.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VolumeProfile {
    /// Inclusive low / high range used to size the bins.
    pub price_low: Decimal,
    pub price_high: Decimal,
    pub bin_size: Decimal,
    /// Bins ordered low → high.
    pub bins: Vec<VPBin>,
    /// Volume Point of Control — bin midpoint with the largest volume.
    pub vpoc: Decimal,
    /// Value Area High / Low — smallest contiguous band around VPOC
    /// containing `cfg.value_area_pct` of total volume.
    pub vah: Decimal,
    pub val: Decimal,
    /// High Volume Node midpoints (local maxima). Empty if profile too
    /// flat to produce meaningful peaks.
    pub hvns: Vec<Decimal>,
    /// Low Volume Node midpoints (local minima / vacuums).
    pub lvns: Vec<Decimal>,
    /// Total traded volume across all bins.
    pub total_volume: Decimal,
}

impl VolumeProfile {
    /// Pure derivation step shared by every builder. Given a bin vector
    /// and the config, fills VPOC / VAH / VAL / HVN / LVN. Builders
    /// should always call this so the post-processing logic lives in
    /// one place (CLAUDE.md rule #1 — no duplicated branches per
    /// builder).
    pub fn derive(
        price_low: Decimal,
        price_high: Decimal,
        bin_size: Decimal,
        bins: Vec<VPBin>,
        cfg: &VProfileConfig,
    ) -> Self {
        let total_volume: Decimal = bins.iter().map(|b| b.volume).sum();
        // VPOC — bin with max volume.
        let vpoc_idx = bins
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.volume.cmp(&b.volume))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let vpoc = bins
            .get(vpoc_idx)
            .map(|b| b.mid())
            .unwrap_or(price_low);

        // Value Area expansion — start at VPOC, add the larger of the
        // two adjacent neighbours until we hit `value_area_pct × total`.
        let target = total_volume
            * Decimal::from_f64_retain(cfg.value_area_pct).unwrap_or(Decimal::ZERO);
        let (mut lo, mut hi) = (vpoc_idx, vpoc_idx);
        let mut acc = bins.get(vpoc_idx).map(|b| b.volume).unwrap_or(Decimal::ZERO);
        while acc < target && (lo > 0 || hi < bins.len().saturating_sub(1)) {
            let lower_vol = if lo > 0 {
                bins[lo - 1].volume
            } else {
                Decimal::from(-1) // sentinel: ineligible
            };
            let upper_vol = if hi < bins.len() - 1 {
                bins[hi + 1].volume
            } else {
                Decimal::from(-1)
            };
            // Pick the heavier neighbour; ties favour upper (auction-
            // theory convention).
            let take_upper = upper_vol >= lower_vol;
            if take_upper && upper_vol >= Decimal::ZERO {
                hi += 1;
                acc += bins[hi].volume;
            } else if lower_vol >= Decimal::ZERO {
                lo -= 1;
                acc += bins[lo].volume;
            } else {
                break;
            }
        }
        let vah = bins.get(hi).map(|b| b.price_high).unwrap_or(price_high);
        let val = bins.get(lo).map(|b| b.price_low).unwrap_or(price_low);

        // HVN / LVN via local-window prominence (CLAUDE.md #1 — single
        // pass, no per-pattern branching).
        let half = cfg.local_neighbourhood_half_width;
        let mut hvns = Vec::new();
        let mut lvns = Vec::new();
        for (i, bin) in bins.iter().enumerate() {
            let lo_n = i.saturating_sub(half);
            let hi_n = (i + half).min(bins.len() - 1);
            if hi_n - lo_n < 2 {
                continue;
            }
            let neighbour_vol: Decimal = (lo_n..=hi_n)
                .filter(|&j| j != i)
                .map(|j| bins[j].volume)
                .sum();
            let neighbour_count = (hi_n - lo_n) as u64;
            if neighbour_count == 0 {
                continue;
            }
            let mean = neighbour_vol / Decimal::from(neighbour_count);
            if mean <= Decimal::ZERO {
                continue;
            }
            let ratio = bin.volume / mean;
            let hvn_thr = Decimal::from_f64_retain(cfg.hvn_min_prominence_pct)
                .unwrap_or(Decimal::from(1));
            let lvn_thr = Decimal::from_f64_retain(cfg.lvn_max_pct)
                .unwrap_or(Decimal::ZERO);
            if ratio >= hvn_thr {
                hvns.push(bin.mid());
            } else if ratio <= lvn_thr {
                lvns.push(bin.mid());
            }
        }

        Self {
            price_low,
            price_high,
            bin_size,
            bins,
            vpoc,
            vah,
            val,
            hvns,
            lvns,
            total_volume,
        }
    }

    /// Returns the HVN closest to `price` in the given direction.
    /// `up = true` → smallest HVN > price; `up = false` → largest HVN < price.
    pub fn nearest_hvn(&self, price: Decimal, up: bool) -> Option<Decimal> {
        self.hvns
            .iter()
            .copied()
            .filter(|h| if up { *h > price } else { *h < price })
            .min_by(|a, b| {
                let da = (*a - price).abs();
                let db = (*b - price).abs();
                da.cmp(&db)
            })
    }
}
