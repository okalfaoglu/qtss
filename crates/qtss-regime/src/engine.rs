//! Regime engine — wires the four indicators to the classifier.
//!
//! Streaming: feed bars one at a time, get back `Some(RegimeSnapshot)`
//! once every indicator has finished its warm-up window. Until then
//! the engine reports `None` rather than emitting a half-formed verdict.

use crate::adx::AdxState;
use crate::bbands::BBandsState;
use crate::choppiness::ChoppinessState;
use crate::classifier::{classify, Indicators};
use crate::config::RegimeConfig;
use crate::error::{RegimeError, RegimeResult};
use chrono::{DateTime, Utc};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::regime::RegimeSnapshot;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

pub struct RegimeEngine {
    config: RegimeConfig,
    adx: AdxState,
    bbands: BBandsState,
    chop: ChoppinessState,
    /// Simple TR-based ATR%, computed inline rather than dragging in
    /// qtss-pivots — keeps the dependency graph minimal.
    atr_period: usize,
    atr_sum: f64,
    atr_seen: usize,
    atr: Option<f64>,
    prev_close: Option<f64>,
    bar_index: u64,
    last_time: Option<DateTime<Utc>>,
    last_snapshot: Option<RegimeSnapshot>,
}

impl RegimeEngine {
    pub fn new(config: RegimeConfig) -> RegimeResult<Self> {
        config.validate()?;
        Ok(Self {
            adx: AdxState::new(config.adx_period),
            bbands: BBandsState::new(config.bb_period, config.bb_stddev),
            chop: ChoppinessState::new(config.chop_period),
            atr_period: config.adx_period,
            atr_sum: 0.0,
            atr_seen: 0,
            atr: None,
            prev_close: None,
            bar_index: 0,
            last_time: None,
            last_snapshot: None,
            config,
        })
    }

    pub fn snapshot(&self) -> Option<RegimeSnapshot> {
        self.last_snapshot.clone()
    }

    pub fn on_bar(&mut self, bar: &Bar) -> RegimeResult<Option<RegimeSnapshot>> {
        let idx = self.bar_index;
        self.bar_index += 1;
        if let Some(prev) = self.last_time {
            if bar.open_time < prev {
                return Err(RegimeError::NonMonotonic(idx));
            }
        }
        self.last_time = Some(bar.open_time);

        let high = decimal_to_f64(bar.high);
        let low = decimal_to_f64(bar.low);
        let close = decimal_to_f64(bar.close);

        // ATR (simple Wilder for the same period as ADX).
        self.update_atr(high, low, close);

        let adx_reading = self.adx.update(high, low, close);
        let bb_reading = self.bbands.update(close);
        let chop_reading = self.chop.update(high, low, close);

        // Need every indicator + a positive close before classifying.
        let (Some(adx), Some(bb), Some(chop), Some(atr)) =
            (adx_reading, bb_reading, chop_reading, self.atr)
        else {
            return Ok(None);
        };
        if close <= 0.0 {
            return Ok(None);
        }
        let atr_pct = atr / close;

        let ind = Indicators {
            adx: adx.adx,
            plus_di: adx.plus_di,
            minus_di: adx.minus_di,
            bb_width: bb.width,
            atr_pct,
            choppiness: chop,
        };
        let verdict = classify(&ind, &self.config);

        let snap = RegimeSnapshot {
            at: bar.open_time,
            kind: verdict.kind,
            trend_strength: verdict.trend_strength,
            adx: f64_to_decimal(adx.adx),
            bb_width: f64_to_decimal(bb.width),
            atr_pct: f64_to_decimal(atr_pct),
            choppiness: f64_to_decimal(chop),
            confidence: verdict.confidence,
        };
        self.last_snapshot = Some(snap.clone());
        Ok(Some(snap))
    }

    fn update_atr(&mut self, high: f64, low: f64, close: f64) {
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
        self.atr_seen += 1;
        if self.atr_seen <= self.atr_period {
            self.atr_sum += tr;
            if self.atr_seen == self.atr_period {
                self.atr = Some(self.atr_sum / self.atr_period as f64);
            }
            return;
        }
        if let Some(prev) = self.atr {
            let p = self.atr_period as f64;
            self.atr = Some((prev * (p - 1.0) + tr) / p);
        }
    }
}

fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

fn f64_to_decimal(f: f64) -> Decimal {
    // Indicator outputs comfortably fit; fall back to zero on NaN/inf so
    // the snapshot stays serializable.
    Decimal::from_f64_retain(f).unwrap_or_default()
}
