//! Choppiness Index.
//!
//!   CI = 100 * log10(sum(TR, n) / (max(high, n) - min(low, n))) / log10(n)
//!
//! Output is roughly 0..100. Higher = more sideways/range, lower = more
//! trending. We compute it over a sliding `period` window of bars.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy)]
struct WindowBar {
    high: f64,
    low: f64,
    tr: f64,
}

#[derive(Debug, Clone)]
pub struct ChoppinessState {
    period: usize,
    log_period: f64,
    bars: VecDeque<WindowBar>,
    prev_close: Option<f64>,
    last: Option<f64>,
}

impl ChoppinessState {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            log_period: (period as f64).log10(),
            bars: VecDeque::with_capacity(period),
            prev_close: None,
            last: None,
        }
    }

    pub fn value(&self) -> Option<f64> {
        self.last
    }

    pub fn update(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        let tr = match self.prev_close {
            None => high - low,
            Some(pc) => {
                let hl = high - low;
                let hc = (high - pc).abs();
                let lc = (low - pc).abs();
                hl.max(hc).max(lc)
            }
        };
        self.prev_close = Some(close);

        if self.bars.len() == self.period {
            self.bars.pop_front();
        }
        self.bars.push_back(WindowBar { high, low, tr });

        if self.bars.len() < self.period {
            return None;
        }
        let sum_tr: f64 = self.bars.iter().map(|b| b.tr).sum();
        let max_hi = self
            .bars
            .iter()
            .map(|b| b.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_lo = self
            .bars
            .iter()
            .map(|b| b.low)
            .fold(f64::INFINITY, f64::min);
        let range = max_hi - min_lo;
        if range <= 0.0 || sum_tr <= 0.0 {
            return None;
        }
        let ci = 100.0 * (sum_tr / range).log10() / self.log_period;
        self.last = Some(ci);
        Some(ci)
    }
}
