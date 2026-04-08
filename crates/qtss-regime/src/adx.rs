//! Wilder ADX with directional indicators (+DI / -DI).
//!
//! Standard textbook formulation:
//!   * TR  = max(high-low, |high-prev_close|, |low-prev_close|)
//!   * +DM = max(high - prev_high, 0) if it exceeds (prev_low - low), else 0
//!   * -DM = max(prev_low - low, 0)   if it exceeds (high - prev_high), else 0
//!   * +DI = 100 * smoothed(+DM) / smoothed(TR)
//!   * -DI = 100 * smoothed(-DM) / smoothed(TR)
//!   * DX  = 100 * |+DI - -DI| / (+DI + -DI)
//!   * ADX = Wilder-smoothed DX
//!
//! All math is f64 — these are statistical aggregates and Decimal sqrt/div
//! would buy nothing measurable for the consumers (regime classification).

#[derive(Debug, Clone, Copy, Default)]
pub struct AdxReading {
    pub adx: f64,
    pub plus_di: f64,
    pub minus_di: f64,
}

#[derive(Debug, Clone)]
pub struct AdxState {
    period: usize,
    period_f: f64,
    seen: usize,
    prev_high: Option<f64>,
    prev_low: Option<f64>,
    prev_close: Option<f64>,
    /// Wilder-smoothed sums.
    smoothed_tr: f64,
    smoothed_plus_dm: f64,
    smoothed_minus_dm: f64,
    /// ADX is itself Wilder-smoothed over `period` DX values; we keep a
    /// running sum during the ADX warm-up window then switch to smoothing.
    dx_warmup_sum: f64,
    dx_warmup_count: usize,
    adx: Option<f64>,
    last_reading: Option<AdxReading>,
}

impl AdxState {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            period_f: period as f64,
            seen: 0,
            prev_high: None,
            prev_low: None,
            prev_close: None,
            smoothed_tr: 0.0,
            smoothed_plus_dm: 0.0,
            smoothed_minus_dm: 0.0,
            dx_warmup_sum: 0.0,
            dx_warmup_count: 0,
            adx: None,
            last_reading: None,
        }
    }

    pub fn value(&self) -> Option<AdxReading> {
        self.last_reading
    }

    pub fn update(&mut self, high: f64, low: f64, close: f64) -> Option<AdxReading> {
        let (tr, plus_dm, minus_dm) = self.directional_movement(high, low);
        self.prev_high = Some(high);
        self.prev_low = Some(low);
        self.prev_close = Some(close);
        self.seen += 1;

        // First bar has no previous; we still need it to seed the prev_*.
        if self.seen == 1 {
            return None;
        }

        // Warm-up window: simple sum.
        if self.seen <= self.period + 1 {
            self.smoothed_tr += tr;
            self.smoothed_plus_dm += plus_dm;
            self.smoothed_minus_dm += minus_dm;
            if self.seen == self.period + 1 {
                // Start producing DI on the next bar via Wilder smoothing.
            }
            return None;
        }

        // Wilder smoothing step.
        self.smoothed_tr = self.smoothed_tr - (self.smoothed_tr / self.period_f) + tr;
        self.smoothed_plus_dm =
            self.smoothed_plus_dm - (self.smoothed_plus_dm / self.period_f) + plus_dm;
        self.smoothed_minus_dm =
            self.smoothed_minus_dm - (self.smoothed_minus_dm / self.period_f) + minus_dm;

        if self.smoothed_tr <= 0.0 {
            return None;
        }
        let plus_di = 100.0 * self.smoothed_plus_dm / self.smoothed_tr;
        let minus_di = 100.0 * self.smoothed_minus_dm / self.smoothed_tr;
        let denom = plus_di + minus_di;
        if denom <= 0.0 {
            return None;
        }
        let dx = 100.0 * (plus_di - minus_di).abs() / denom;

        let adx = match self.adx {
            None => {
                self.dx_warmup_sum += dx;
                self.dx_warmup_count += 1;
                if self.dx_warmup_count == self.period {
                    let seed = self.dx_warmup_sum / self.period_f;
                    self.adx = Some(seed);
                    seed
                } else {
                    return None;
                }
            }
            Some(prev) => {
                let next = (prev * (self.period_f - 1.0) + dx) / self.period_f;
                self.adx = Some(next);
                next
            }
        };

        let reading = AdxReading {
            adx,
            plus_di,
            minus_di,
        };
        self.last_reading = Some(reading);
        Some(reading)
    }

    fn directional_movement(&self, high: f64, low: f64) -> (f64, f64, f64) {
        let prev_close = match self.prev_close {
            Some(v) => v,
            None => return (high - low, 0.0, 0.0),
        };
        let prev_high = self.prev_high.unwrap_or(high);
        let prev_low = self.prev_low.unwrap_or(low);

        let hl = high - low;
        let hc = (high - prev_close).abs();
        let lc = (low - prev_close).abs();
        let tr = hl.max(hc).max(lc);

        let up_move = high - prev_high;
        let down_move = prev_low - low;
        let plus_dm = if up_move > down_move && up_move > 0.0 {
            up_move
        } else {
            0.0
        };
        let minus_dm = if down_move > up_move && down_move > 0.0 {
            down_move
        } else {
            0.0
        };
        (tr, plus_dm, minus_dm)
    }
}
