//! Per-family validator impls. Each impl receives a snapshot of the
//! detection row + current price + current ATR + the shared config.
//! Returns `Invalidate` when the pattern's geometry is broken, else
//! `Hold` (the writer then leaves `invalidated = false` alone).

use crate::config::ValidatorConfig;
use crate::registry::DetectionRow;
use crate::verdict::{InvalidationReason, ValidatorVerdict};
use serde_json::Value;

pub trait Validator: Send + Sync {
    fn family(&self) -> &'static str;
    fn validate(
        &self,
        row: &DetectionRow,
        price: f64,
        atr: f64,
        cfg: &ValidatorConfig,
    ) -> (ValidatorVerdict, Option<InvalidationReason>);
}

fn raw_meta_f64(row: &DetectionRow, key: &str) -> Option<f64> {
    row.raw_meta.get(key).and_then(|v| match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    })
}

fn anchor_by_label(row: &DetectionRow, label: &str) -> Option<f64> {
    row.anchors
        .as_array()?
        .iter()
        .find(|a| {
            a.get("label_override")
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case(label))
                .unwrap_or(false)
                || a.get("label")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case(label))
                    .unwrap_or(false)
        })
        .and_then(|a| a.get("price"))
        .and_then(|v| v.as_f64())
}

// ── Harmonic ──────────────────────────────────────────────────────────

pub struct HarmonicValidator;
impl Validator for HarmonicValidator {
    fn family(&self) -> &'static str {
        "harmonic"
    }
    fn validate(
        &self,
        row: &DetectionRow,
        price: f64,
        _atr: f64,
        cfg: &ValidatorConfig,
    ) -> (ValidatorVerdict, Option<InvalidationReason>) {
        let Some(d) = anchor_by_label(row, "D") else {
            return (ValidatorVerdict::Hold, None);
        };
        let Some(x) = anchor_by_label(row, "X") else {
            return (ValidatorVerdict::Hold, None);
        };
        let Some(a) = anchor_by_label(row, "A") else {
            return (ValidatorVerdict::Hold, None);
        };
        let xa = (a - x).abs();
        let tol = xa * cfg.harmonic_break_pct;
        let broken = if row.direction >= 0 {
            // Bullish harmonic — D is a low; break = close below D - tol.
            price < d - tol
        } else {
            price > d + tol
        };
        if broken {
            (ValidatorVerdict::Invalidate, Some(InvalidationReason::GeometryBroken))
        } else {
            (ValidatorVerdict::Hold, None)
        }
    }
}

// ── Classical (generic via `raw_meta.invalidation`) ───────────────────

pub struct ClassicalValidator;
impl Validator for ClassicalValidator {
    fn family(&self) -> &'static str {
        "classical"
    }
    fn validate(
        &self,
        row: &DetectionRow,
        price: f64,
        _atr: f64,
        cfg: &ValidatorConfig,
    ) -> (ValidatorVerdict, Option<InvalidationReason>) {
        let Some(inv) = raw_meta_f64(row, "invalidation") else {
            return (ValidatorVerdict::Hold, None);
        };
        let tol = price.abs() * cfg.classical_break_pct;
        let broken = if row.direction >= 0 {
            price < inv - tol
        } else {
            price > inv + tol
        };
        if broken {
            (ValidatorVerdict::Invalidate, Some(InvalidationReason::GeometryBroken))
        } else {
            (ValidatorVerdict::Hold, None)
        }
    }
}

// ── Range (zone fully traversed) ──────────────────────────────────────

pub struct RangeValidator;
impl Validator for RangeValidator {
    fn family(&self) -> &'static str {
        "range"
    }
    fn validate(
        &self,
        row: &DetectionRow,
        price: f64,
        _atr: f64,
        cfg: &ValidatorConfig,
    ) -> (ValidatorVerdict, Option<InvalidationReason>) {
        // Zone bounds come from anchors (gap_high/gap_low, ob_high/ob_low,
        // or the first two anchors generically).
        let anchors = row.anchors.as_array();
        let Some(anchors) = anchors else {
            return (ValidatorVerdict::Hold, None);
        };
        if anchors.len() < 2 {
            return (ValidatorVerdict::Hold, None);
        }
        let p0 = anchors[0].get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let p1 = anchors[1].get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let top = p0.max(p1);
        let bot = p0.min(p1);
        let height = (top - bot).max(1e-9);
        let traversed = if row.direction >= 0 {
            (price - bot) / height
        } else {
            (top - price) / height
        };
        if traversed >= cfg.range_full_fill_pct {
            (ValidatorVerdict::Invalidate, Some(InvalidationReason::ZoneFilled))
        } else {
            (ValidatorVerdict::Hold, None)
        }
    }
}

// ── Gap (close across the gap) ────────────────────────────────────────

pub struct GapValidator;
impl Validator for GapValidator {
    fn family(&self) -> &'static str {
        "gap"
    }
    fn validate(
        &self,
        row: &DetectionRow,
        price: f64,
        _atr: f64,
        cfg: &ValidatorConfig,
    ) -> (ValidatorVerdict, Option<InvalidationReason>) {
        let Some(inv) = raw_meta_f64(row, "invalidation_price") else {
            return (ValidatorVerdict::Hold, None);
        };
        let closed = if row.direction >= 0 {
            price <= inv
        } else {
            price >= inv
        };
        // For bull gaps: closed when price returns to gap_bar.low;
        // gap_close_pct lets a sliver of noise before flagging.
        let margin = inv * (1.0 - cfg.gap_close_pct);
        let broken = if row.direction >= 0 {
            price <= inv + margin.abs()
        } else {
            price >= inv - margin.abs()
        };
        if closed || broken {
            (ValidatorVerdict::Invalidate, Some(InvalidationReason::GapClosed))
        } else {
            (ValidatorVerdict::Hold, None)
        }
    }
}

// ── Motive (Elliott wave-1 break) ─────────────────────────────────────

pub struct MotiveValidator;
impl Validator for MotiveValidator {
    fn family(&self) -> &'static str {
        "motive"
    }
    fn validate(
        &self,
        row: &DetectionRow,
        price: f64,
        _atr: f64,
        cfg: &ValidatorConfig,
    ) -> (ValidatorVerdict, Option<InvalidationReason>) {
        let Some(anchors) = row.anchors.as_array() else {
            return (ValidatorVerdict::Hold, None);
        };
        // Motive anchors = [0, 1, 2, 3, 4, 5] — wave 1 peak = anchors[1].
        let Some(wave1) = anchors.get(1).and_then(|a| a.get("price")).and_then(|v| v.as_f64())
        else {
            return (ValidatorVerdict::Hold, None);
        };
        let buf = wave1.abs() * cfg.motive_wave1_buffer_pct;
        let broken = if row.direction >= 0 {
            price < wave1 - buf
        } else {
            price > wave1 + buf
        };
        if broken {
            (ValidatorVerdict::Invalidate, Some(InvalidationReason::StructuralBreak))
        } else {
            (ValidatorVerdict::Hold, None)
        }
    }
}

// ── SMC (event's invalidation_price from raw_meta) ───────────────────

pub struct SmcValidator;
impl Validator for SmcValidator {
    fn family(&self) -> &'static str {
        "smc"
    }
    fn validate(
        &self,
        row: &DetectionRow,
        price: f64,
        _atr: f64,
        cfg: &ValidatorConfig,
    ) -> (ValidatorVerdict, Option<InvalidationReason>) {
        let Some(inv) = raw_meta_f64(row, "invalidation_price") else {
            return (ValidatorVerdict::Hold, None);
        };
        let buf = inv.abs() * cfg.smc_break_buffer_pct;
        let broken = if row.direction >= 0 {
            price < inv - buf
        } else {
            price > inv + buf
        };
        if broken {
            (ValidatorVerdict::Invalidate, Some(InvalidationReason::GeometryBroken))
        } else {
            (ValidatorVerdict::Hold, None)
        }
    }
}

// ── ORB (close back inside OR) ───────────────────────────────────────

pub struct OrbValidator;
impl Validator for OrbValidator {
    fn family(&self) -> &'static str {
        "orb"
    }
    fn validate(
        &self,
        row: &DetectionRow,
        price: f64,
        _atr: f64,
        _cfg: &ValidatorConfig,
    ) -> (ValidatorVerdict, Option<InvalidationReason>) {
        let Some(or_high) = raw_meta_f64(row, "or_high") else {
            return (ValidatorVerdict::Hold, None);
        };
        let Some(or_low) = raw_meta_f64(row, "or_low") else {
            return (ValidatorVerdict::Hold, None);
        };
        // If price has re-entered the OR after the breakout bar, it's
        // a fakeout.
        let back_inside = price > or_low && price < or_high;
        if back_inside {
            (ValidatorVerdict::Invalidate, Some(InvalidationReason::ReEntryFakeout))
        } else {
            (ValidatorVerdict::Hold, None)
        }
    }
}
