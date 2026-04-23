//! Event evaluators — one `fn` per SMC kind. Every evaluator has the
//! same signature so the detector loop is a single iteration over
//! [`SMC_SPECS`] with no per-kind branch (CLAUDE.md #1).

use crate::config::SmcConfig;
use crate::event::{SmcEvent, SmcEventKind};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::pivot::{Pivot, PivotKind};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

fn dec(x: f64) -> Decimal {
    Decimal::from_f64(x).unwrap_or(Decimal::ZERO)
}

fn price_f64(p: &Pivot) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    p.price.to_f64().unwrap_or(0.0)
}

fn bar_close(b: &Bar) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    b.close.to_f64().unwrap_or(0.0)
}

fn bar_high(b: &Bar) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    b.high.to_f64().unwrap_or(0.0)
}

fn bar_low(b: &Bar) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    b.low.to_f64().unwrap_or(0.0)
}

// ── BOS — Break of Structure (continuation) ────────────────────────────
//
// Bullish BOS: close > prior swing high, AND the prior swing high was
// itself the most recent high (no CHoCH between).
// Bearish BOS: mirror with lows.

pub fn eval_bos(pivots: &[Pivot], bars: &[Bar], cfg: &SmcConfig) -> Vec<SmcEvent> {
    let mut out = Vec::new();
    if pivots.len() < 4 || bars.is_empty() {
        return out;
    }
    let scan_start = bars.len().saturating_sub(cfg.scan_lookback);
    for bar_idx in scan_start..bars.len() {
        let b = &bars[bar_idx];
        let close = bar_close(b);
        // Last pivot _before_ this bar. Walk pivots backwards.
        let bars_time = b.open_time;
        let prior: Vec<&Pivot> = pivots
            .iter()
            .filter(|p| p.time < bars_time)
            .collect();
        if prior.len() < 3 {
            continue;
        }
        // Bullish BOS: price breaks most recent high which itself was HH.
        if let Some(last_high) = prior.iter().rev().find(|p| p.kind == PivotKind::High) {
            let prev_highs: Vec<&&Pivot> = prior
                .iter()
                .rev()
                .filter(|p| p.kind == PivotKind::High)
                .take(3)
                .collect();
            if prev_highs.len() >= 2 {
                let hh_pre = price_f64(prev_highs[1]);
                let hh_last = price_f64(last_high);
                if hh_last > hh_pre && close > hh_last {
                    out.push(SmcEvent {
                        kind: SmcEventKind::Bos,
                        variant: "bull",
                        score: 0.70,
                        bar_index: bar_idx,
                        reference_price: last_high.price,
                        invalidation_price: prev_highs[1].price,
                    });
                }
            }
        }
        // Bearish BOS.
        if let Some(last_low) = prior.iter().rev().find(|p| p.kind == PivotKind::Low) {
            let prev_lows: Vec<&&Pivot> = prior
                .iter()
                .rev()
                .filter(|p| p.kind == PivotKind::Low)
                .take(3)
                .collect();
            if prev_lows.len() >= 2 {
                let ll_pre = price_f64(prev_lows[1]);
                let ll_last = price_f64(last_low);
                if ll_last < ll_pre && close < ll_last {
                    out.push(SmcEvent {
                        kind: SmcEventKind::Bos,
                        variant: "bear",
                        score: 0.70,
                        bar_index: bar_idx,
                        reference_price: last_low.price,
                        invalidation_price: prev_lows[1].price,
                    });
                }
            }
        }
    }
    out
}

// ── CHoCH — Change of Character (reversal) ────────────────────────────
//
// Bearish CHoCH: price breaks the most recent *low* while the preceding
// swing structure was making HHs. Bullish mirror.

pub fn eval_choch(pivots: &[Pivot], bars: &[Bar], cfg: &SmcConfig) -> Vec<SmcEvent> {
    let mut out = Vec::new();
    if pivots.len() < 4 || bars.is_empty() {
        return out;
    }
    let scan_start = bars.len().saturating_sub(cfg.scan_lookback);
    for bar_idx in scan_start..bars.len() {
        let b = &bars[bar_idx];
        let close = bar_close(b);
        let prior: Vec<&Pivot> = pivots.iter().filter(|p| p.time < b.open_time).collect();
        if prior.len() < 4 {
            continue;
        }
        let highs: Vec<&&Pivot> = prior
            .iter()
            .rev()
            .filter(|p| p.kind == PivotKind::High)
            .take(3)
            .collect();
        let lows: Vec<&&Pivot> = prior
            .iter()
            .rev()
            .filter(|p| p.kind == PivotKind::Low)
            .take(3)
            .collect();
        // Bearish CHoCH — was making HHs, now breaks most recent low.
        if highs.len() >= 2 && lows.len() >= 1 {
            let was_hh = price_f64(highs[0]) > price_f64(highs[1]);
            if was_hh && close < price_f64(lows[0]) {
                out.push(SmcEvent {
                    kind: SmcEventKind::Choch,
                    variant: "bear",
                    score: 0.75,
                    bar_index: bar_idx,
                    reference_price: lows[0].price,
                    invalidation_price: highs[0].price,
                });
            }
        }
        // Bullish CHoCH — was making LLs, now breaks most recent high.
        if lows.len() >= 2 && highs.len() >= 1 {
            let was_ll = price_f64(lows[0]) < price_f64(lows[1]);
            if was_ll && close > price_f64(highs[0]) {
                out.push(SmcEvent {
                    kind: SmcEventKind::Choch,
                    variant: "bull",
                    score: 0.75,
                    bar_index: bar_idx,
                    reference_price: highs[0].price,
                    invalidation_price: lows[0].price,
                });
            }
        }
    }
    out
}

// ── MSS — Market Structure Shift (stricter CHoCH) ─────────────────────
//
// Same geometry as CHoCH but the break must clear the prior swing by
// `mss_close_cushion_pct`. A CHoCH that barely nicks the level is a
// fakeout risk; MSS filters for follow-through.

pub fn eval_mss(pivots: &[Pivot], bars: &[Bar], cfg: &SmcConfig) -> Vec<SmcEvent> {
    let raw = eval_choch(pivots, bars, cfg);
    raw.into_iter()
        .filter_map(|mut ev| {
            let bar = bars.get(ev.bar_index)?;
            let close = bar_close(bar);
            let ref_p = {
                use rust_decimal::prelude::ToPrimitive;
                ev.reference_price.to_f64().unwrap_or(0.0)
            };
            let cushion = ref_p * cfg.mss_close_cushion_pct;
            let ok = match ev.variant {
                "bull" => close >= ref_p + cushion,
                "bear" => close <= ref_p - cushion,
                _ => false,
            };
            if !ok {
                return None;
            }
            ev.kind = SmcEventKind::Mss;
            ev.score = 0.80;
            Some(ev)
        })
        .collect()
}

// ── Liquidity Sweep — wick + rejection ────────────────────────────────
//
// Buy-side sweep (bear signal): wick pokes above a prior swing high by
// >= `sweep_wick_penetration_pct`, then close recovers below the swing
// high within `sweep_reject_bars`. Sell-side mirror.

pub fn eval_liquidity_sweep(pivots: &[Pivot], bars: &[Bar], cfg: &SmcConfig) -> Vec<SmcEvent> {
    let mut out = Vec::new();
    if pivots.is_empty() || bars.is_empty() {
        return out;
    }
    let scan_start = bars.len().saturating_sub(cfg.scan_lookback);
    for bar_idx in scan_start..bars.len() {
        let b = &bars[bar_idx];
        let prior: Vec<&Pivot> = pivots.iter().filter(|p| p.time < b.open_time).collect();
        // Buy-side: sweep a prior swing high.
        if let Some(last_high) = prior.iter().rev().find(|p| p.kind == PivotKind::High) {
            let h = price_f64(last_high);
            let penetration = bar_high(b) - h;
            if penetration > h * cfg.sweep_wick_penetration_pct {
                // Look at the next few bars (including current) for
                // rejection close.
                let end_rej = (bar_idx + cfg.sweep_reject_bars).min(bars.len() - 1);
                let mut rejected = false;
                for k in bar_idx..=end_rej {
                    let recover = bar_high(b) - bar_close(&bars[k]);
                    if recover >= penetration * cfg.sweep_reject_frac && bar_close(&bars[k]) < h {
                        rejected = true;
                        break;
                    }
                }
                if rejected {
                    out.push(SmcEvent {
                        kind: SmcEventKind::LiquiditySweep,
                        variant: "bear",
                        score: 0.72,
                        bar_index: bar_idx,
                        reference_price: last_high.price,
                        invalidation_price: dec(bar_high(b)),
                    });
                }
            }
        }
        // Sell-side.
        if let Some(last_low) = prior.iter().rev().find(|p| p.kind == PivotKind::Low) {
            let l = price_f64(last_low);
            let penetration = l - bar_low(b);
            if penetration > l * cfg.sweep_wick_penetration_pct {
                let end_rej = (bar_idx + cfg.sweep_reject_bars).min(bars.len() - 1);
                let mut rejected = false;
                for k in bar_idx..=end_rej {
                    let recover = bar_close(&bars[k]) - bar_low(b);
                    if recover >= penetration * cfg.sweep_reject_frac && bar_close(&bars[k]) > l {
                        rejected = true;
                        break;
                    }
                }
                if rejected {
                    out.push(SmcEvent {
                        kind: SmcEventKind::LiquiditySweep,
                        variant: "bull",
                        score: 0.72,
                        bar_index: bar_idx,
                        reference_price: last_low.price,
                        invalidation_price: dec(bar_low(b)),
                    });
                }
            }
        }
    }
    out
}

// ── FVI — Fair Value Imbalance (strict FVG cousin) ────────────────────
//
// Bullish: candle[i-2].high < candle[i].low, volume[i-1] > spike mult.
// Bearish: mirror.

pub fn eval_fvi(_pivots: &[Pivot], bars: &[Bar], cfg: &SmcConfig) -> Vec<SmcEvent> {
    let mut out = Vec::new();
    if bars.len() < 23 {
        return out;
    }
    // ATR via simple range-average over last 14 bars per position.
    let scan_start = bars.len().saturating_sub(cfg.scan_lookback).max(20);
    for i in scan_start..bars.len() {
        if i < 2 {
            continue;
        }
        let prev_prev = &bars[i - 2];
        let mid = &bars[i - 1];
        let curr = &bars[i];
        // Compute a short ATR (5-bar range mean) around the mid to
        // judge the gap magnitude.
        let lo = i.saturating_sub(5);
        let atr: f64 = bars[lo..i]
            .iter()
            .map(|b| bar_high(b) - bar_low(b))
            .sum::<f64>()
            / (i - lo).max(1) as f64;
        if atr <= 0.0 {
            continue;
        }
        // Volume spike gate — middle-candle volume vs SMA-20 baseline.
        use rust_decimal::prelude::ToPrimitive;
        let sma_start = i.saturating_sub(20);
        let sma_vol: f64 = bars[sma_start..i]
            .iter()
            .map(|b| b.volume.to_f64().unwrap_or(0.0))
            .sum::<f64>()
            / (i - sma_start).max(1) as f64;
        let mid_vol = mid.volume.to_f64().unwrap_or(0.0);
        if sma_vol > 0.0 && mid_vol < sma_vol * cfg.fvi_volume_spike_mult {
            continue;
        }
        // Bullish FVI: c1.high < c3.low.
        let h1 = bar_high(prev_prev);
        let l3 = bar_low(curr);
        if l3 > h1 {
            let gap = l3 - h1;
            if gap > atr * cfg.fvi_min_gap_atr_frac {
                out.push(SmcEvent {
                    kind: SmcEventKind::Fvi,
                    variant: "bull",
                    score: 0.65,
                    bar_index: i - 1,
                    reference_price: dec((h1 + l3) / 2.0),
                    invalidation_price: dec(h1),
                });
            }
        }
        // Bearish FVI: c1.low > c3.high.
        let l1 = bar_low(prev_prev);
        let h3 = bar_high(curr);
        if l1 > h3 {
            let gap = l1 - h3;
            if gap > atr * cfg.fvi_min_gap_atr_frac {
                out.push(SmcEvent {
                    kind: SmcEventKind::Fvi,
                    variant: "bear",
                    score: 0.65,
                    bar_index: i - 1,
                    reference_price: dec((l1 + h3) / 2.0),
                    invalidation_price: dec(l1),
                });
            }
        }
    }
    out
}
