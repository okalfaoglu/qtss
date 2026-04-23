//! Derivatives event evaluators — one `fn` per kind. All share the
//! same "(snapshot_json, config) → Vec<DerivEvent>" contract so the
//! engine writer loops over them uniformly (CLAUDE.md #1).

use crate::config::DerivConfig;
use crate::event::{DerivEvent, DerivEventKind};
use serde_json::Value;

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

fn std_dev(xs: &[f64], m: f64) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let var: f64 = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() - 1) as f64;
    var.sqrt()
}

// ── FundingSpike — array payload from /fapi/v1/fundingRate ────────────
//
// JSON: `[{ fundingRate, fundingTime, markPrice, symbol }, ...]`
// Last element = latest period.

pub fn detect_funding_spike(payload: &Value, cfg: &DerivConfig) -> Vec<DerivEvent> {
    let mut out = Vec::new();
    let Some(arr) = payload.as_array() else { return out; };
    if arr.len() < 5 {
        return out;
    }
    let series: Vec<f64> = arr
        .iter()
        .filter_map(|o| o.get("fundingRate").and_then(as_f64))
        .collect();
    if series.len() < 5 {
        return out;
    }
    let window_start = series.len().saturating_sub(cfg.funding_window + 1);
    let window = &series[window_start..series.len() - 1]; // exclude current
    let latest = *series.last().unwrap();
    let m = mean(window);
    let sd = std_dev(window, m);
    if sd <= 0.0 {
        return out;
    }
    let z = (latest - m) / sd;
    if z.abs() < cfg.funding_z_threshold {
        return out;
    }
    // Extreme positive funding → longs paying shorts heavily → market
    // over-long → contrarian bear signal. Inverse for negative.
    let (variant, note): (&'static str, String) = if z > 0.0 {
        ("bear", format!("Aşırı pozitif funding (z={z:.2}σ) — longlar ücretlendiriliyor, pozisyon yoğunluğu kaldıraç baskısına yol açabilir"))
    } else {
        ("bull", format!("Aşırı negatif funding (z={z:.2}σ) — shortlar ücretlendiriliyor, short squeeze potansiyeli"))
    };
    let score = (z.abs() / cfg.funding_z_threshold).min(3.0) / 3.0 * 0.9 + 0.1;
    out.push(DerivEvent {
        kind: DerivEventKind::FundingSpike,
        variant,
        score,
        metric_value: latest,
        baseline_value: m,
        note,
    });
    out
}

// ── OIImbalance — Binance /futures/data/openInterestHist payload ──────
//
// Expected shape (from the aggregated history endpoint):
//   [{ sumOpenInterest, sumOpenInterestValue, timestamp }, ...]
// We also accept the simple snapshot `{ openInterest, time }` single
// object: in that case we can only emit based on a caller-supplied
// `baseline_oi` (so the single-shot endpoint isn't enough on its own
// — falls back silently).

pub fn detect_oi_imbalance(
    payload: &Value,
    price_delta_pct: f64,
    cfg: &DerivConfig,
) -> Vec<DerivEvent> {
    let mut out = Vec::new();
    let (current, baseline) = match payload {
        Value::Array(arr) if !arr.is_empty() => {
            let first = arr
                .first()
                .and_then(|o| {
                    o.get("sumOpenInterest")
                        .or_else(|| o.get("openInterest"))
                })
                .and_then(as_f64);
            let last = arr
                .last()
                .and_then(|o| {
                    o.get("sumOpenInterest")
                        .or_else(|| o.get("openInterest"))
                })
                .and_then(as_f64);
            (last, first)
        }
        _ => return out,
    };
    let (Some(cur), Some(base)) = (current, baseline) else { return out; };
    if base <= 0.0 {
        return out;
    }
    let delta = (cur - base) / base;
    if delta.abs() < cfg.oi_delta_pct && price_delta_pct.abs() < cfg.oi_price_divergence_pct {
        return out;
    }
    // Imbalance: OI up but price flat/down (traders pressing short into
    // weakness, or over-leveraging ahead of reversal) → bear. Mirror:
    // OI up + price up = trend continuation (not imbalance). OI down +
    // price up = short-covering (bull).
    let (variant, note): (&'static str, String) = match (delta > 0.0, price_delta_pct > 0.0) {
        (true, false) => (
            "bear",
            format!("OI %{:.1} arttı ama fiyat %{:.1} hareketi — yeni pozisyonlar baskı altında", delta * 100.0, price_delta_pct * 100.0),
        ),
        (false, true) => (
            "bull",
            format!("OI %{:.1} düştü, fiyat %{:.1} yükseldi — short-covering", delta * 100.0, price_delta_pct * 100.0),
        ),
        _ => return out, // Continuation, not imbalance.
    };
    let score = (delta.abs() / cfg.oi_delta_pct).min(3.0) / 3.0 * 0.8 + 0.2;
    out.push(DerivEvent {
        kind: DerivEventKind::OiImbalance,
        variant,
        score,
        metric_value: delta,
        baseline_value: cfg.oi_delta_pct,
        note,
    });
    out
}

// ── BasisDislocation — /fapi/v1/premiumIndex payload ──────────────────
//
// JSON: `{ markPrice, indexPrice, lastFundingRate, ... }`
// Basis % = (mark - index) / index.

pub fn detect_basis_dislocation(payload: &Value, cfg: &DerivConfig) -> Vec<DerivEvent> {
    let mut out = Vec::new();
    let mark = payload.get("markPrice").and_then(as_f64);
    let index = payload.get("indexPrice").and_then(as_f64);
    let (Some(mark), Some(index)) = (mark, index) else { return out; };
    if index <= 0.0 {
        return out;
    }
    let basis = (mark - index) / index;
    if basis.abs() < cfg.basis_dislocation_pct {
        return out;
    }
    // Perp trading above spot → longs paying premium → contrarian bear
    // when extreme. Inverse for below spot.
    let (variant, note): (&'static str, String) = if basis > 0.0 {
        ("bear", format!("Perp spota göre +%{:.3} prim ile trade oluyor — long spekülatif yoğunluk", basis * 100.0))
    } else {
        ("bull", format!("Perp spota göre %{:.3} iskonto ile trade oluyor — short spekülatif yoğunluk", basis * 100.0))
    };
    let score = (basis.abs() / cfg.basis_dislocation_pct).min(5.0) / 5.0 * 0.8 + 0.2;
    out.push(DerivEvent {
        kind: DerivEventKind::BasisDislocation,
        variant,
        score,
        metric_value: basis,
        baseline_value: 0.0,
        note,
    });
    out
}

// ── LongShortRatioExtreme — /futures/data/globalLongShortAccountRatio ─
//
// JSON (array of): `{ longShortRatio, longAccount, shortAccount, ... }`
// Latest entry is the most recent.

pub fn detect_long_short_extreme(payload: &Value, cfg: &DerivConfig) -> Vec<DerivEvent> {
    let mut out = Vec::new();
    let arr = match payload {
        Value::Array(a) => a,
        _ => return out,
    };
    let Some(latest) = arr.last() else { return out; };
    let ratio = latest.get("longShortRatio").and_then(as_f64);
    let Some(r) = ratio else { return out; };
    if r <= 0.0 {
        return out;
    }
    // ratio > 1 means long-heavy; < 1 means short-heavy.
    let (variant, extreme, note): (&'static str, f64, String) = if r >= cfg.lsr_long_extreme {
        (
            "bear",
            cfg.lsr_long_extreme,
            format!("Long/Short oranı {r:.2} — aşırı kalabalık long, contrarian bearish"),
        )
    } else if r <= 1.0 / cfg.lsr_short_extreme {
        (
            "bull",
            1.0 / cfg.lsr_short_extreme,
            format!("Long/Short oranı {r:.2} — aşırı kalabalık short, short squeeze potansiyeli"),
        )
    } else {
        return out;
    };
    let score = {
        let over = if r >= 1.0 {
            r / cfg.lsr_long_extreme
        } else {
            cfg.lsr_short_extreme * r
        };
        (over.max(1.0) - 1.0).min(2.0) / 2.0 * 0.7 + 0.3
    };
    out.push(DerivEvent {
        kind: DerivEventKind::LongShortExtreme,
        variant,
        score,
        metric_value: r,
        baseline_value: extreme,
        note,
    });
    out
}

// ── TakerFlowImbalance — /futures/data/takerlongshortRatio ────────────
//
// JSON (array): `{ buySellRatio, buyVol, sellVol, timestamp }`
// buySellRatio ~ 1 = neutral; > 1 = buy-dominant; < 1 = sell-dominant.

pub fn detect_taker_flow_imbalance(payload: &Value, cfg: &DerivConfig) -> Vec<DerivEvent> {
    let mut out = Vec::new();
    let arr = match payload {
        Value::Array(a) => a,
        _ => return out,
    };
    let Some(latest) = arr.last() else { return out; };
    let ratio = latest.get("buySellRatio").and_then(as_f64);
    let Some(r) = ratio else { return out; };
    if r <= 0.0 {
        return out;
    }
    let (variant, threshold, note): (&'static str, f64, String) = if r >= cfg.taker_buy_dominance {
        (
            "bull",
            cfg.taker_buy_dominance,
            format!("Taker buy/sell {r:.2} — alıcı hakimiyeti güçlü"),
        )
    } else if r <= 1.0 / cfg.taker_sell_dominance {
        (
            "bear",
            1.0 / cfg.taker_sell_dominance,
            format!("Taker buy/sell {r:.2} — satıcı hakimiyeti güçlü"),
        )
    } else {
        return out;
    };
    let over = if r >= 1.0 {
        r / cfg.taker_buy_dominance
    } else {
        cfg.taker_sell_dominance * r
    };
    let score = (over.max(1.0) - 1.0).min(1.5) / 1.5 * 0.7 + 0.3;
    out.push(DerivEvent {
        kind: DerivEventKind::TakerFlowImbalance,
        variant,
        score,
        metric_value: r,
        baseline_value: threshold,
        note,
    });
    out
}
