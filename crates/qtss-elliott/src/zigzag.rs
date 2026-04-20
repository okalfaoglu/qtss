//! ZigZag detector (LuxAlgo 1:1 port).
//!
//! Dynamically detects pivot points (high/low alternation) from a bar stream
//! using configurable lookback windows. Implements the LuxAlgo Elliott Wave
//! zigzag logic:
//!   * When a new pivot high found after a low (or vice versa), add it.
//!   * If in the same direction, replace the last point with a more extreme value.
//!   * Maintains a fixed-size rolling buffer of recent pivots.

use qtss_domain::v2::bar::Bar;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Direction: 1 = most recent pivot is HIGH, -1 = most recent pivot is LOW.
type Direction = i8;
const DIR_HIGH: Direction = 1;
const DIR_LOW: Direction = -1;

/// A pivot point in the zigzag stream.
#[derive(Debug, Clone)]
pub struct ZigZagPoint {
    /// Bars ago (0 = current bar, 1 = prior bar, etc.)
    pub bars_ago: usize,
    /// The extreme price.
    pub price: f64,
    /// Direction: 1 (HIGH) or -1 (LOW).
    pub direction: Direction,
}

/// Rolling zigzag state — maintains the detected pivot sequence.
#[derive(Debug, Clone)]
pub struct ZigZag {
    /// Recent pivots (newest at index 0).
    points: VecDeque<ZigZagPoint>,
    /// Current direction (last detected pivot type).
    current_direction: Direction,
    /// Maximum capacity (like Pine Script's fixed 11-element array).
    max_capacity: usize,
}

impl ZigZag {
    pub fn new(capacity: usize) -> Self {
        Self {
            points: VecDeque::with_capacity(capacity),
            current_direction: 0, // neutral, no pivot yet
            max_capacity: capacity,
        }
    }

    /// Get all recent points (newest first).
    pub fn points(&self) -> &VecDeque<ZigZagPoint> {
        &self.points
    }

    /// Age all points by incrementing bars_ago (call when new bar arrives).
    pub fn age_points(&mut self) {
        for point in self.points.iter_mut() {
            point.bars_ago += 1;
        }
    }

    /// Add a new pivot or replace existing if same direction.
    pub fn add_or_replace(&mut self, bars_ago: usize, price: f64, direction: Direction) {
        if self.current_direction == direction && !self.points.is_empty() {
            // Same direction: update the last point if more extreme.
            if let Some(last) = self.points.front_mut() {
                let is_more_extreme = if direction == DIR_HIGH {
                    price > last.price
                } else {
                    price < last.price
                };
                if is_more_extreme {
                    last.price = price;
                    last.bars_ago = bars_ago;
                }
            }
        } else {
            // Direction changed: add new pivot.
            self.points.push_front(ZigZagPoint {
                bars_ago,
                price,
                direction,
            });
            self.current_direction = direction;

            // Maintain max capacity (FIFO for oldest).
            if self.points.len() > self.max_capacity {
                self.points.pop_back();
            }
        }
    }

    /// Get the last N points in chronological order (oldest first).
    pub fn recent(&self, n: usize) -> Vec<ZigZagPoint> {
        self.points
            .iter()
            .take(n)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Get the most recent point.
    pub fn last(&self) -> Option<&ZigZagPoint> {
        self.points.front()
    }
}

/// Detects if there is a pivot high within a lookback window.
/// Returns Some if found, None otherwise. Uses ta.pivothigh logic:
///   - Must be highest within [current - lookback .. current]
///   - Must be >= next bar's high (forward-looking)
fn detect_pivot_high(bars: &[Bar], lookback: usize) -> bool {
    if bars.is_empty() || lookback == 0 {
        return false;
    }
    let current_idx = bars.len() - 1;
    if current_idx < lookback {
        return false; // Not enough history
    }

    let current_high = bars[current_idx].high.to_f64().unwrap_or(0.0);

    // Forward check: if next bar exists, current must not be lower.
    let next_high = if current_idx + 1 < bars.len() {
        bars[current_idx + 1].high.to_f64().unwrap_or(f64::NEG_INFINITY)
    } else {
        f64::NEG_INFINITY
    };

    if current_high < next_high {
        return false;
    }

    // Lookback check: must be highest in [current - lookback .. current].
    let start = current_idx.saturating_sub(lookback);
    bars[start..=current_idx]
        .iter()
        .all(|b| current_high >= b.high.to_f64().unwrap_or(0.0))
}

/// Detects if there is a pivot low within a lookback window.
/// Returns Some if found, None otherwise. Uses ta.pivotlow logic:
///   - Must be lowest within [current - lookback .. current]
///   - Must be <= next bar's low (forward-looking)
fn detect_pivot_low(bars: &[Bar], lookback: usize) -> bool {
    if bars.is_empty() || lookback == 0 {
        return false;
    }
    let current_idx = bars.len() - 1;
    if current_idx < lookback {
        return false;
    }

    let current_low = bars[current_idx].low.to_f64().unwrap_or(0.0);

    // Forward check: if next bar exists, current must not be higher.
    let next_low = if current_idx + 1 < bars.len() {
        bars[current_idx + 1].low.to_f64().unwrap_or(f64::INFINITY)
    } else {
        f64::INFINITY
    };

    if current_low > next_low {
        return false;
    }

    // Lookback check: must be lowest in [current - lookback .. current].
    let start = current_idx.saturating_sub(lookback);
    bars[start..=current_idx]
        .iter()
        .all(|b| current_low <= b.low.to_f64().unwrap_or(0.0))
}

/// Process bar stream: age existing points, detect new pivots, update zigzag state.
/// Call once per new bar.
pub fn process_bar(
    zigzag: &mut ZigZag,
    bars: &[Bar],
    lookback_high: usize,
    lookback_low: usize,
) {
    // Age existing points (each bar moves them further into history).
    zigzag.age_points();

    // Check for new pivot high.
    if detect_pivot_high(bars, lookback_high) {
        let price = bars[bars.len() - 1].high.to_f64().unwrap_or(0.0);
        zigzag.add_or_replace(0, price, DIR_HIGH);
    }

    // Check for new pivot low.
    if detect_pivot_low(bars, lookback_low) {
        let price = bars[bars.len() - 1].low.to_f64().unwrap_or(0.0);
        zigzag.add_or_replace(0, price, DIR_LOW);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
    use qtss_domain::v2::timeframe::Timeframe;
    use chrono::Utc;
    use rust_decimal_macros::dec;

    fn mock_bar(high: f64, low: f64, close: f64) -> Bar {
        Bar {
            instrument: Instrument {
                symbol: "TEST".to_string(),
                venue: Venue::Binance,
                asset_class: AssetClass::Spot,
                quote_asset: "USDT".to_string(),
                session_calendar: SessionCalendar::Crypto24h,
            },
            timeframe: Timeframe::M1,
            open_time: Utc::now(),
            open: dec!(close),
            high: dec!(high),
            low: dec!(low),
            close: dec!(close),
            volume: dec!(100),
            closed: true,
        }
    }

    #[test]
    fn test_detect_pivot_high() {
        // Simple uptrend with pivot high at peak
        let bars = vec![
            mock_bar(100.0, 95.0, 99.0),
            mock_bar(105.0, 100.0, 104.0),
            mock_bar(110.0, 105.0, 109.0),
            mock_bar(108.0, 103.0, 105.0), // Peak at 110, now declining
        ];
        assert!(detect_pivot_high(&bars, 3));
    }

    #[test]
    fn test_detect_pivot_low() {
        let bars = vec![
            mock_bar(100.0, 95.0, 96.0),
            mock_bar(95.0, 90.0, 91.0),
            mock_bar(92.0, 85.0, 88.0),
            mock_bar(95.0, 88.0, 92.0), // Trough at 85, now rising
        ];
        assert!(detect_pivot_low(&bars, 3));
    }

    #[test]
    fn test_zigzag_alternation() {
        let mut zz = ZigZag::new(10);
        zz.add_or_replace(0, 100.0, DIR_HIGH);
        assert_eq!(zz.current_direction, DIR_HIGH);
        assert_eq!(zz.points().len(), 1);

        zz.add_or_replace(0, 90.0, DIR_LOW);
        assert_eq!(zz.current_direction, DIR_LOW);
        assert_eq!(zz.points().len(), 2);

        zz.add_or_replace(0, 110.0, DIR_HIGH);
        assert_eq!(zz.current_direction, DIR_HIGH);
        assert_eq!(zz.points().len(), 3);
    }

    #[test]
    fn test_zigzag_same_direction_replacement() {
        let mut zz = ZigZag::new(10);
        zz.add_or_replace(0, 100.0, DIR_HIGH);
        zz.add_or_replace(0, 110.0, DIR_HIGH); // More extreme, should replace

        assert_eq!(zz.points().len(), 1);
        assert_eq!(zz.last().unwrap().price, 110.0);
    }

    #[test]
    fn test_age_points() {
        let mut zz = ZigZag::new(10);
        zz.add_or_replace(0, 100.0, DIR_HIGH);
        zz.age_points();
        assert_eq!(zz.last().unwrap().bars_ago, 1);

        zz.age_points();
        assert_eq!(zz.last().unwrap().bars_ago, 2);
    }
}
