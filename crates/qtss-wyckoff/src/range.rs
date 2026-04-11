//! Trading-range geometry helpers.
//!
//! Wyckoff analysis is built on the idea of a horizontal "trading range"
//! bounded by a support line and a resistance line. We compute that box
//! from the trailing pivots and reuse it across every Wyckoff event
//! (range itself, Spring, Upthrust, …).

use qtss_domain::v2::pivot::{Pivot, PivotKind};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy)]
pub struct TradingRange {
    pub support: f64,
    pub resistance: f64,
    pub mid: f64,
    pub height: f64,
}

impl TradingRange {
    /// Build a range from a slice of pivots. Returns `None` if the slice
    /// has fewer than two highs or two lows, or if the resulting box is
    /// degenerate.
    pub fn from_pivots(pivots: &[Pivot]) -> Option<Self> {
        let mut highs: Vec<f64> = Vec::new();
        let mut lows: Vec<f64> = Vec::new();
        for p in pivots {
            let v = p.price.to_f64()?;
            match p.kind {
                PivotKind::High => highs.push(v),
                PivotKind::Low => lows.push(v),
            }
        }
        if highs.len() < 2 || lows.len() < 2 {
            return None;
        }
        // Use the median-ish bounds: mean of the two extremes on each side.
        // This is robust to a single pivot poking out (which is exactly
        // what a Spring / Upthrust looks like — and we want the *body* of
        // the range, not the false break).
        highs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        lows.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let resistance = mean_top(&highs);
        let support = mean_bottom(&lows);
        if !(resistance > support) {
            return None;
        }
        let mid = (resistance + support) / 2.0;
        let height = resistance - support;
        Some(Self {
            support,
            resistance,
            mid,
            height,
        })
    }

    /// How well the pivots are clustered against the box edges. 1.0 = all
    /// highs/lows hug the boundary, 0.0 = scattered.
    pub fn edge_tightness(&self, pivots: &[Pivot], tolerance: f64) -> Option<f64> {
        let mut score_sum = 0.0;
        let mut count = 0;
        for p in pivots {
            let v = p.price.to_f64()?;
            // Distance to the *nearest* edge as a fraction of range height.
            let d = (v - self.support).abs().min((v - self.resistance).abs());
            let pct = d / self.height.max(1e-9);
            // Gaussian fall-off: pct=0 -> 1.0, pct=tolerance -> ~0.6
            let z = pct / tolerance.max(1e-9);
            score_sum += (-(z * z) / 2.0).exp();
            count += 1;
        }
        if count == 0 {
            return None;
        }
        Some(score_sum / count as f64)
    }
}

fn mean_top(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n >= 2 {
        (sorted[n - 1] + sorted[n - 2]) / 2.0
    } else {
        sorted[n - 1]
    }
}

fn mean_bottom(sorted: &[f64]) -> f64 {
    if sorted.len() >= 2 {
        (sorted[0] + sorted[1]) / 2.0
    } else {
        sorted[0]
    }
}

impl TradingRange {
    /// Compute the slope of the range via simple linear regression on
    /// all pivot prices vs their bar_index. Returns degrees (-90..+90).
    /// Positive = rising range, negative = falling range.
    pub fn slope_degrees(&self, pivots: &[Pivot]) -> Option<f64> {
        if pivots.len() < 3 {
            return None;
        }
        // Linear regression: y = price, x = bar_index
        let n = pivots.len() as f64;
        let mut sum_x = 0.0_f64;
        let mut sum_y = 0.0_f64;
        let mut sum_xy = 0.0_f64;
        let mut sum_x2 = 0.0_f64;
        for p in pivots {
            let x = p.bar_index as f64;
            let y = p.price.to_f64()?;
            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_x2 += x * x;
        }
        let denom = n * sum_x2 - sum_x * sum_x;
        if denom.abs() < 1e-12 {
            return None;
        }
        let slope = (n * sum_xy - sum_x * sum_y) / denom;
        // Normalize slope relative to range height to get meaningful degrees
        let normalized = slope * (pivots.len() as f64) / self.height.max(1e-9);
        Some(normalized.atan().to_degrees())
    }

    /// Returns true if the range is "sloping" (above threshold degrees).
    pub fn is_sloping(&self, pivots: &[Pivot], threshold_deg: f64) -> bool {
        self.slope_degrees(pivots)
            .map(|d| d.abs() > threshold_deg)
            .unwrap_or(false)
    }
}

/// Average pivot volume over the slice. Used as the baseline against
/// which a single pivot's volume is compared to declare it climactic.
pub fn average_volume(pivots: &[Pivot]) -> Option<Decimal> {
    if pivots.is_empty() {
        return None;
    }
    let mut sum = Decimal::ZERO;
    for p in pivots {
        sum += p.volume_at_pivot;
    }
    Some(sum / Decimal::from(pivots.len() as i64))
}
