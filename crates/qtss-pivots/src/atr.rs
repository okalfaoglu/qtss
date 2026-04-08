//! Incremental ATR (Average True Range).
//!
//! Wilder's smoothing — the canonical formulation used by every charting
//! package. We compute it incrementally so the engine can stream bars
//! without re-walking history on every tick.
//!
//! Decimal arithmetic throughout so the same value is reproducible across
//! Rust/Postgres/JSON serialization without float drift.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Debug, Clone)]
pub struct AtrState {
    period: usize,
    period_dec: Decimal,
    seen: usize,
    /// Sum of TR over the warm-up window before Wilder smoothing kicks in.
    warmup_sum: Decimal,
    /// Last known close — needed to compute TR for the next bar.
    prev_close: Option<Decimal>,
    current: Option<Decimal>,
}

impl AtrState {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            period_dec: Decimal::from(period as i64),
            seen: 0,
            warmup_sum: dec!(0),
            prev_close: None,
            current: None,
        }
    }

    /// Current ATR, or `None` until the warm-up window has been filled.
    pub fn value(&self) -> Option<Decimal> {
        self.current
    }

    /// Feed one bar (high, low, close). Returns the new ATR if available.
    /// We always store the close so the *next* bar's TR can include the
    /// gap-aware terms even before the average is ready.
    pub fn update(&mut self, high: Decimal, low: Decimal, close: Decimal) -> Option<Decimal> {
        let tr = self.true_range(high, low);
        self.prev_close = Some(close);
        self.seen += 1;

        if self.seen <= self.period {
            self.warmup_sum += tr;
            if self.seen == self.period {
                self.current = Some(self.warmup_sum / self.period_dec);
            }
            return self.current;
        }

        // Wilder smoothing: ATR = ((period-1) * prev + tr) / period
        let prev = self.current.expect("warm-up complete by this point");
        let next = (prev * (self.period_dec - dec!(1)) + tr) / self.period_dec;
        self.current = Some(next);
        self.current
    }

    fn true_range(&self, high: Decimal, low: Decimal) -> Decimal {
        let hl = high - low;
        match self.prev_close {
            None => hl,
            Some(pc) => {
                let hc = (high - pc).abs();
                let lc = (low - pc).abs();
                max3(hl, hc, lc)
            }
        }
    }
}

fn max3(a: Decimal, b: Decimal, c: Decimal) -> Decimal {
    let ab = if a > b { a } else { b };
    if ab > c {
        ab
    } else {
        c
    }
}
