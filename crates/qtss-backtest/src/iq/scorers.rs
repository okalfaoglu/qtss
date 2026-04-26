//! Time-bounded reimplementations of every Major-Dip / Major-Top
//! composite scorer. Same matrices and decay shapes as the live
//! `score_*` functions in `qtss-worker::major_dip_candidate_loop`,
//! but each query is bounded by `bar_time` so historical replay
//! never sees future data.
//!
//! Live parity goal: when this backtest runs over the SAME
//! historical bars the live worker has already processed, every
//! channel returns the SAME number it returned live. That keeps
//! optimisation honest — weight tweaks here will translate to live
//! behaviour 1:1.
//!
//! 8 channels covered (10 total minus wyckoff_alignment +
//! cycle_alignment which already live in `detector.rs`):
//!
//!   1. structural_completion
//!   2. fib_retrace_quality
//!   3. volume_capitulation
//!   4. cvd_divergence
//!   5. indicator_alignment
//!   6. sentiment_extreme
//!   7. multi_tf_confluence
//!   8. funding_oi_signals

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::{PgPool, Row};

use super::config::IqPolarity;

/// Convenience — keys the scorers query by, mirrors live `SymbolKey`.
#[derive(Debug, Clone)]
pub struct ScoreKey<'a> {
    pub exchange: &'a str,
    pub segment: &'a str,
    pub symbol: &'a str,
    pub timeframe: &'a str,
}

/// 12.1 — Structural completion. Reads the iq_structures row
/// FROZEN at `bar_time` (the most recent advance ≤ that time).
pub async fn structural_completion(
    pool: &PgPool,
    k: &ScoreKey<'_>,
    bar_time: DateTime<Utc>,
) -> f64 {
    let row = sqlx::query(
        r#"SELECT current_wave, state, raw_meta
             FROM iq_structures
            WHERE exchange=$1 AND segment=$2 AND symbol=$3
              AND timeframe=$4
              AND state IN ('candidate','tracking','completed')
              AND last_advanced_at <= $5
            ORDER BY last_advanced_at DESC LIMIT 1"#,
    )
    .bind(k.exchange).bind(k.segment).bind(k.symbol).bind(k.timeframe)
    .bind(bar_time)
    .fetch_optional(pool).await.ok().flatten();
    let Some(r) = row else { return 0.0; };
    let cw: String = r.try_get("current_wave").unwrap_or_default();
    let state: String = r.try_get("state").unwrap_or_default();
    let meta: Value = r.try_get("raw_meta").unwrap_or(Value::Null);
    let primary_branch = meta
        .get("projection")
        .and_then(|p| p.get("primary_branch"))
        .and_then(|b| b.as_str());
    let branch_score: f64 = match primary_branch {
        Some(kind) => meta
            .get("projection")
            .and_then(|p| p.get("branches"))
            .and_then(|b| b.as_array())
            .and_then(|arr| {
                arr.iter().find(|x| {
                    x.get("kind").and_then(|kk| kk.as_str()) == Some(kind)
                })
            })
            .and_then(|b| b.get("score").and_then(|s| s.as_f64()))
            .unwrap_or(0.0),
        None => 0.0,
    };
    match (cw.as_str(), state.as_str()) {
        ("C", "completed") => 1.0,
        ("C", "tracking") => 0.7 * branch_score,
        ("B", _) => 0.4 * branch_score,
        ("A", _) => 0.2,
        ("W3", _) | ("W4", _) | ("W5", _) => 0.1,
        _ => 0.0,
    }
}

/// 12.2 — Fib retracement quality. Reads pivots up to bar_time and
/// scores how close `bar_close` is to a canonical Fib level (0.382 /
/// 0.500 / 0.618 / 0.786) of the last 200-bar L2 swing.
pub async fn fib_retrace_quality(
    pool: &PgPool,
    k: &ScoreKey<'_>,
    bar_time: DateTime<Utc>,
    bar_close: Decimal,
) -> f64 {
    let row = sqlx::query(
        r#"SELECT MAX(p.price) AS hi, MIN(p.price) AS lo
             FROM pivots p
             JOIN engine_symbols es ON es.id = p.engine_symbol_id
            WHERE es.exchange=$1 AND es.segment=$2
              AND es.symbol=$3 AND es.interval=$4
              AND p.level = 2
              AND p.open_time <= $5
              AND p.open_time >= $5 - INTERVAL '200 days'"#,
    )
    .bind(k.exchange).bind(k.segment).bind(k.symbol).bind(k.timeframe)
    .bind(bar_time)
    .fetch_optional(pool).await.ok().flatten();
    let Some(r) = row else { return 0.0; };
    let hi: Option<Decimal> = r.try_get("hi").ok();
    let lo: Option<Decimal> = r.try_get("lo").ok();
    let hi_f = hi.and_then(|d| d.to_f64()).unwrap_or(0.0);
    let lo_f = lo.and_then(|d| d.to_f64()).unwrap_or(0.0);
    let close_f = bar_close.to_f64().unwrap_or(0.0);
    if hi_f <= lo_f || hi_f - lo_f < 1e-9 {
        return 0.0;
    }
    let retrace_pct = (hi_f - close_f) / (hi_f - lo_f);
    let mut best = 0.0_f64;
    for r in [0.382_f64, 0.500, 0.618, 0.786] {
        let d = (retrace_pct - r).abs();
        let s = if d <= 0.025 {
            1.0
        } else if d <= 0.075 {
            1.0 - (d - 0.025) / 0.05
        } else {
            0.0
        };
        if s > best {
            best = s;
        }
    }
    best.clamp(0.0, 1.0)
}

/// 12.3 — Volume capitulation (Wyckoff SC heuristic, polarity-aware).
pub async fn volume_capitulation(
    pool: &PgPool,
    k: &ScoreKey<'_>,
    bar_time: DateTime<Utc>,
    polarity: IqPolarity,
) -> f64 {
    let rows = sqlx::query(
        r#"SELECT high, low, close, volume
             FROM market_bars
            WHERE exchange=$1 AND segment=$2 AND symbol=$3
              AND interval=$4
              AND open_time <= $5
            ORDER BY open_time DESC LIMIT 20"#,
    )
    .bind(k.exchange).bind(k.segment).bind(k.symbol).bind(k.timeframe)
    .bind(bar_time)
    .fetch_all(pool).await.unwrap_or_default();
    if rows.len() < 14 {
        return 0.0;
    }
    let mut highs = Vec::with_capacity(rows.len());
    let mut lows = Vec::with_capacity(rows.len());
    let mut closes = Vec::with_capacity(rows.len());
    let mut vols = Vec::with_capacity(rows.len());
    for r in &rows {
        let h: Decimal = r.try_get("high").unwrap_or_default();
        let l: Decimal = r.try_get("low").unwrap_or_default();
        let c: Decimal = r.try_get("close").unwrap_or_default();
        let v: Decimal = r.try_get("volume").unwrap_or_default();
        highs.push(h.to_f64().unwrap_or(0.0));
        lows.push(l.to_f64().unwrap_or(0.0));
        closes.push(c.to_f64().unwrap_or(0.0));
        vols.push(v.to_f64().unwrap_or(0.0));
    }
    let mut tr_total = 0.0;
    for i in 0..14usize.min(highs.len()) {
        tr_total += (highs[i] - lows[i]).abs();
    }
    let atr = tr_total / 14.0;
    if atr < 1e-9 {
        return 0.0;
    }
    let climax_vol = vols.iter().copied().fold(0.0_f64, f64::max);
    let mut sorted_vols = vols.clone();
    sorted_vols.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let baseline_vol = sorted_vols
        .get(sorted_vols.len() / 2)
        .copied()
        .unwrap_or(1.0)
        .max(1.0);
    let climax_ratio = climax_vol / baseline_vol;
    let climax_idx = vols
        .iter()
        .position(|v| (*v - climax_vol).abs() < 1e-9)
        .unwrap_or(0);
    let climax_range = highs[climax_idx] - lows[climax_idx];
    let climax_range_atr = climax_range / atr;
    let shadow_ratio = if climax_range > 1e-9 {
        match polarity {
            IqPolarity::Dip => {
                (closes[climax_idx] - lows[climax_idx]) / climax_range
            }
            IqPolarity::Top => {
                (highs[climax_idx] - closes[climax_idx]) / climax_range
            }
        }
    } else {
        0.0
    };
    let ratio_score = ((climax_ratio - 1.5) / 1.5).clamp(0.0, 1.0);
    let range_score = ((climax_range_atr - 1.0) / 1.0).clamp(0.0, 1.0);
    let shadow_score = ((shadow_ratio - 0.5).max(0.0)).min(0.5) * 2.0;
    0.4 * ratio_score + 0.3 * range_score + 0.3 * shadow_score
}

/// 12.4 — CVD bullish divergence (Dip) / bearish divergence (Top).
/// Compares price low-vs-low (or high-vs-high) with cumulative volume
/// delta over the trailing 60 bars. CVD column lives in
/// `bar_indicator_cvd` if it exists; v1 falls back to volume-only
/// proxy when CVD is missing.
pub async fn cvd_divergence(
    pool: &PgPool,
    k: &ScoreKey<'_>,
    bar_time: DateTime<Utc>,
    polarity: IqPolarity,
) -> f64 {
    // Try the dedicated CVD snapshot table first; fall back to a
    // volume-imbalance proxy. Live parity exact bar-for-bar requires
    // the same CVD source — backtest will write its own CVD into a
    // staging table in 26.4, for now we use the proxy.
    let rows = sqlx::query(
        r#"SELECT close, volume
             FROM market_bars
            WHERE exchange=$1 AND segment=$2 AND symbol=$3
              AND interval=$4
              AND open_time <= $5
            ORDER BY open_time DESC LIMIT 60"#,
    )
    .bind(k.exchange).bind(k.segment).bind(k.symbol).bind(k.timeframe)
    .bind(bar_time)
    .fetch_all(pool).await.unwrap_or_default();
    if rows.len() < 30 {
        return 0.0;
    }
    let mut closes = Vec::with_capacity(rows.len());
    let mut vols = Vec::with_capacity(rows.len());
    for r in &rows {
        let c: Decimal = r.try_get("close").unwrap_or_default();
        let v: Decimal = r.try_get("volume").unwrap_or_default();
        closes.push(c.to_f64().unwrap_or(0.0));
        vols.push(v.to_f64().unwrap_or(0.0));
    }
    // Reverse so closes[0] is oldest, closes[last] is newest.
    closes.reverse();
    vols.reverse();
    let n = closes.len();
    if n < 30 {
        return 0.0;
    }
    // Build proxy CVD: signed volume by close direction.
    let mut cvd = vec![0.0; n];
    cvd[0] = vols[0];
    for i in 1..n {
        let direction = if closes[i] > closes[i - 1] {
            1.0
        } else if closes[i] < closes[i - 1] {
            -1.0
        } else {
            0.0
        };
        cvd[i] = cvd[i - 1] + direction * vols[i];
    }
    // Find the recent extremum (last 10 bars) and an earlier one in
    // bars [n-30..n-10] to compare.
    let recent_end = n;
    let recent_start = recent_end.saturating_sub(10);
    let prior_end = n.saturating_sub(10);
    let prior_start = prior_end.saturating_sub(20);
    if prior_start >= prior_end {
        return 0.0;
    }
    let (recent_extreme_idx, prior_extreme_idx) = match polarity {
        IqPolarity::Dip => {
            // Lowest price in each window.
            let recent_idx = (recent_start..recent_end)
                .min_by(|&i, &j| closes[i].partial_cmp(&closes[j]).unwrap())
                .unwrap_or(recent_start);
            let prior_idx = (prior_start..prior_end)
                .min_by(|&i, &j| closes[i].partial_cmp(&closes[j]).unwrap())
                .unwrap_or(prior_start);
            (recent_idx, prior_idx)
        }
        IqPolarity::Top => {
            let recent_idx = (recent_start..recent_end)
                .max_by(|&i, &j| closes[i].partial_cmp(&closes[j]).unwrap())
                .unwrap_or(recent_start);
            let prior_idx = (prior_start..prior_end)
                .max_by(|&i, &j| closes[i].partial_cmp(&closes[j]).unwrap())
                .unwrap_or(prior_start);
            (recent_idx, prior_idx)
        }
    };
    let p_recent = closes[recent_extreme_idx];
    let p_prior = closes[prior_extreme_idx];
    let cvd_recent = cvd[recent_extreme_idx];
    let cvd_prior = cvd[prior_extreme_idx];
    // Bullish divergence (Dip): price LL but CVD HL → score > 0.
    // Bearish divergence (Top): price HH but CVD LH → score > 0.
    let div_strength = match polarity {
        IqPolarity::Dip => {
            if p_recent < p_prior && cvd_recent > cvd_prior {
                let price_drop = ((p_prior - p_recent) / p_prior.abs().max(1e-9)).abs();
                let cvd_rise = ((cvd_recent - cvd_prior) / cvd_prior.abs().max(1e-9)).abs();
                (price_drop + cvd_rise).min(1.0)
            } else {
                0.0
            }
        }
        IqPolarity::Top => {
            if p_recent > p_prior && cvd_recent < cvd_prior {
                let price_rise = ((p_recent - p_prior) / p_prior.abs().max(1e-9)).abs();
                let cvd_drop = ((cvd_prior - cvd_recent) / cvd_prior.abs().max(1e-9)).abs();
                (price_rise + cvd_drop).min(1.0)
            } else {
                0.0
            }
        }
    };
    div_strength.clamp(0.0, 1.0)
}

/// 12.5 — Indicator alignment (RSI extremes + MACD turn).
/// Reads the most-recent indicator snapshot ≤ bar_time.
pub async fn indicator_alignment(
    pool: &PgPool,
    k: &ScoreKey<'_>,
    bar_time: DateTime<Utc>,
    polarity: IqPolarity,
) -> f64 {
    let row = sqlx::query(
        r#"SELECT data
             FROM bar_indicator_snapshots
            WHERE exchange=$1 AND segment=$2 AND symbol=$3
              AND timeframe=$4
              AND open_time <= $5
            ORDER BY open_time DESC LIMIT 1"#,
    )
    .bind(k.exchange).bind(k.segment).bind(k.symbol).bind(k.timeframe)
    .bind(bar_time)
    .fetch_optional(pool).await.ok().flatten();
    let Some(r) = row else { return 0.0; };
    let data: Value = r.try_get("data").unwrap_or(Value::Null);
    let rsi = data.get("rsi").and_then(|v| v.as_f64()).unwrap_or(50.0);
    let macd_hist = data
        .get("macd_hist")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    // RSI score: oversold for Dip (RSI < 30 → 1.0, RSI = 50 → 0.0).
    let rsi_score = match polarity {
        IqPolarity::Dip => ((30.0 - rsi).max(0.0) / 30.0).clamp(0.0, 1.0),
        IqPolarity::Top => ((rsi - 70.0).max(0.0) / 30.0).clamp(0.0, 1.0),
    };
    // MACD: positive histogram for Dip turn-up, negative for Top.
    let macd_score = match polarity {
        IqPolarity::Dip => {
            if macd_hist > 0.0 {
                (macd_hist / 100.0).clamp(0.0, 1.0)
            } else {
                0.0
            }
        }
        IqPolarity::Top => {
            if macd_hist < 0.0 {
                (-macd_hist / 100.0).clamp(0.0, 1.0)
            } else {
                0.0
            }
        }
    };
    0.6 * rsi_score + 0.4 * macd_score
}

/// 12.6 — Sentiment extreme (Fear & Greed snapshot ≤ bar_time).
pub async fn sentiment_extreme(
    pool: &PgPool,
    bar_time: DateTime<Utc>,
    polarity: IqPolarity,
) -> f64 {
    let row = sqlx::query(
        r#"SELECT value
             FROM fear_greed_snapshots
            WHERE captured_at <= $1
            ORDER BY captured_at DESC LIMIT 1"#,
    )
    .bind(bar_time)
    .fetch_optional(pool).await.ok().flatten();
    let Some(r) = row else { return 0.0; };
    let val: i64 = r.try_get("value").unwrap_or(50);
    let v = val as f64;
    match polarity {
        // Dip wants extreme fear (low values).
        IqPolarity::Dip => ((25.0 - v).max(0.0) / 25.0).clamp(0.0, 1.0),
        // Top wants extreme greed (high values).
        IqPolarity::Top => ((v - 75.0).max(0.0) / 25.0).clamp(0.0, 1.0),
    }
}

/// 12.7 — Multi-timeframe confluence. Counts how many of the
/// related-TF iq_structures rows are in a compatible state at
/// bar_time. Score = matches / total_checked. v1 simple version —
/// 26.4 will weight by TF importance.
pub async fn multi_tf_confluence(
    pool: &PgPool,
    k: &ScoreKey<'_>,
    bar_time: DateTime<Utc>,
    polarity: IqPolarity,
) -> f64 {
    // Look at the structures across all timeframes for this symbol;
    // count how many have a state aligned with the polarity.
    let rows = sqlx::query(
        r#"SELECT timeframe, current_wave, state
             FROM iq_structures
            WHERE exchange=$1 AND segment=$2 AND symbol=$3
              AND last_advanced_at <= $4
              AND state IN ('candidate','tracking','completed')"#,
    )
    .bind(k.exchange).bind(k.segment).bind(k.symbol)
    .bind(bar_time)
    .fetch_all(pool).await.unwrap_or_default();
    if rows.is_empty() {
        return 0.0;
    }
    let mut aligned = 0u32;
    let mut total = 0u32;
    for r in &rows {
        let cw: String = r.try_get("current_wave").unwrap_or_default();
        let state: String = r.try_get("state").unwrap_or_default();
        total += 1;
        let is_aligned = match polarity {
            // Dip aligns when most TFs are in C-completed or W2-W4
            // (corrective region) — i.e. NOT in early markup.
            IqPolarity::Dip => matches!(
                cw.as_str(),
                "C" | "W2" | "W4"
            ) && state != "candidate",
            IqPolarity::Top => matches!(
                cw.as_str(),
                "W5" | "B"
            ),
        };
        if is_aligned {
            aligned += 1;
        }
    }
    if total == 0 {
        0.0
    } else {
        (aligned as f64) / (total as f64)
    }
}

/// 12.8 — Funding/OI signals. Looks at recent perp funding rate +
/// open-interest snapshots ≤ bar_time. Extreme funding rates
/// (positive for top reversal, negative for dip reversal) score 1.0.
pub async fn funding_oi_signals(
    pool: &PgPool,
    k: &ScoreKey<'_>,
    bar_time: DateTime<Utc>,
    polarity: IqPolarity,
) -> f64 {
    // Read the latest snapshot from external_snapshots staging
    // table (key pattern: 'binance_funding_<symbol>'). Fallback 0.0
    // when no historical data — many older bars may pre-date
    // funding ingestion.
    let key = format!("{}_funding_{}", k.exchange, k.symbol.to_lowercase());
    let row = sqlx::query(
        r#"SELECT data
             FROM external_snapshots
            WHERE key=$1 AND fetched_at <= $2
            ORDER BY fetched_at DESC LIMIT 1"#,
    )
    .bind(&key)
    .bind(bar_time)
    .fetch_optional(pool).await.ok().flatten();
    let Some(r) = row else { return 0.0; };
    let data: Value = r.try_get("data").unwrap_or(Value::Null);
    let funding_rate = data
        .get("funding_rate")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    // Threshold: ±0.05% is moderately extreme; ±0.10% is strong.
    let abs_rate = funding_rate.abs();
    let strength = if abs_rate >= 0.001 {
        1.0
    } else if abs_rate >= 0.0005 {
        0.5
    } else {
        0.0
    };
    let aligned = match polarity {
        // Dip wants negative funding (shorts overcrowded, squeeze
        // setup).
        IqPolarity::Dip => funding_rate < 0.0,
        // Top wants positive funding (longs overcrowded).
        IqPolarity::Top => funding_rate > 0.0,
    };
    if aligned {
        strength
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fib_scoring_peaks_at_618() {
        // Fixture-style sanity: a retrace of exactly 0.618 should
        // hit best=1.0 in the inner scoring loop. We simulate by
        // running the same maths inline here (not the async fn).
        let retrace_pct = 0.618_f64;
        let mut best = 0.0_f64;
        for r in [0.382_f64, 0.500, 0.618, 0.786] {
            let d = (retrace_pct - r).abs();
            let s = if d <= 0.025 {
                1.0
            } else if d <= 0.075 {
                1.0 - (d - 0.025) / 0.05
            } else {
                0.0
            };
            if s > best {
                best = s;
            }
        }
        assert!((best - 1.0).abs() < 0.001);
    }

    #[test]
    fn fib_scoring_decays_outside_bands() {
        // 0.45 retrace — between 0.382 and 0.500. Distance to
        // nearest = 0.05, in the linear-decay band.
        let retrace_pct = 0.45_f64;
        let mut best = 0.0_f64;
        for r in [0.382_f64, 0.500, 0.618, 0.786] {
            let d = (retrace_pct - r).abs();
            let s = if d <= 0.025 {
                1.0
            } else if d <= 0.075 {
                1.0 - (d - 0.025) / 0.05
            } else {
                0.0
            };
            if s > best {
                best = s;
            }
        }
        // d=0.05 → s = 1 - (0.05 - 0.025)/0.05 = 0.5
        assert!((best - 0.5).abs() < 0.01);
    }

    #[test]
    fn rsi_oversold_dip_scores_high() {
        // RSI = 20 → ((30 - 20)/30) = 0.333 score → 0.6 * 0.333 = 0.2
        let rsi: f64 = 20.0;
        let s = ((30.0_f64 - rsi).max(0.0_f64) / 30.0_f64).clamp(0.0_f64, 1.0_f64);
        assert!(s > 0.3);
    }

    #[test]
    fn sentiment_extreme_fear_dip_high() {
        // Fear/greed = 10 → extreme fear → ((25-10)/25) = 0.6
        let v: f64 = 10.0;
        let s = ((25.0_f64 - v).max(0.0_f64) / 25.0_f64).clamp(0.0_f64, 1.0_f64);
        assert!((s - 0.6).abs() < 0.01);
    }

    #[test]
    fn funding_aligned_strong_returns_one() {
        // Dip + funding -0.0012 → aligned + |rate|>=0.001 → 1.0
        let funding_rate: f64 = -0.0012;
        let abs_rate = funding_rate.abs();
        let strength: f64 = if abs_rate >= 0.001 {
            1.0
        } else if abs_rate >= 0.0005 {
            0.5
        } else {
            0.0
        };
        let dip_aligned = funding_rate < 0.0;
        assert!(dip_aligned);
        assert!((strength - 1.0_f64).abs() < 0.001);
    }

    #[test]
    fn funding_misaligned_returns_zero() {
        // Dip + positive funding → misaligned → 0.0
        let funding_rate: f64 = 0.0015;
        let dip_aligned = funding_rate < 0.0;
        assert!(!dip_aligned);
    }
}
