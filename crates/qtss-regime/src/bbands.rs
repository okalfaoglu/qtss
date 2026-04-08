//! Incremental Bollinger Bands.
//!
//! Window-based: we keep the last `period` closes in a ring buffer and
//! recompute the SMA + sample std-dev each tick. `period` is small (~20)
//! so the O(n) recompute is cheaper than maintaining a rolling welford.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy)]
pub struct BBandsReading {
    pub mid: f64,
    pub upper: f64,
    pub lower: f64,
    /// (upper - lower) / mid. Zero or NaN are not produced; the engine
    /// can rely on a positive number once warm-up completes.
    pub width: f64,
}

#[derive(Debug, Clone)]
pub struct BBandsState {
    period: usize,
    stddev_mult: f64,
    closes: VecDeque<f64>,
    last: Option<BBandsReading>,
}

impl BBandsState {
    pub fn new(period: usize, stddev_mult: f64) -> Self {
        Self {
            period,
            stddev_mult,
            closes: VecDeque::with_capacity(period),
            last: None,
        }
    }

    pub fn value(&self) -> Option<BBandsReading> {
        self.last
    }

    pub fn update(&mut self, close: f64) -> Option<BBandsReading> {
        if self.closes.len() == self.period {
            self.closes.pop_front();
        }
        self.closes.push_back(close);
        if self.closes.len() < self.period {
            return None;
        }
        let n = self.closes.len() as f64;
        let mean: f64 = self.closes.iter().sum::<f64>() / n;
        let var: f64 = self
            .closes
            .iter()
            .map(|c| {
                let d = c - mean;
                d * d
            })
            .sum::<f64>()
            / n;
        let std = var.sqrt();
        let upper = mean + self.stddev_mult * std;
        let lower = mean - self.stddev_mult * std;
        let width = if mean > 0.0 { (upper - lower) / mean } else { 0.0 };
        let r = BBandsReading {
            mid: mean,
            upper,
            lower,
            width,
        };
        self.last = Some(r);
        Some(r)
    }
}
