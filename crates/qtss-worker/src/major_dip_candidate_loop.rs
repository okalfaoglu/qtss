// FAZ 25.3.A — Major Dip composite scorer.
//
// Reads the spec at docs/MAJOR_DIP_DETECTION_RESEARCH.md §VIII + §XII.
// Each tick we walk every enabled (symbol, timeframe), find the lowest
// recent pivot as the "candidate dip", run 8 component scorers against
// it, and UPSERT the result into `major_dip_candidates`. IQ-D / IQ-T
// candidate loops gate setup creation on the composite score
// (faz 25.3.B, separate PR).
//
// First-iteration coverage:
//   structural_completion  — REAL (reads iq_structures.projection)
//   fib_retrace_quality    — REAL (computes from pivots / market_bars)
//   volume_capitulation    — REAL (Wyckoff SC heuristic on market_bars)
//   cvd_divergence         — STUB (returns 0.0 until cvd_snapshots wired)
//   indicator_alignment    — STUB (returns 0.0 until indicator_snapshots)
//   sentiment_extreme      — REAL (reads fear_greed_snapshots)
//   multi_tf_confluence    — REAL (reads parent-TF iq_structures)
//   funding_oi_signals     — STUB (returns 0.0 until derivatives_snapshots)
//
// Stubs return 0.0 (no signal). Their weight still subtracts from the
// total — composite ≤ 1.0 always. Real scorers replace stubs in a
// follow-up sprint without changing the worker shape.

#![allow(dead_code)]

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

const MODULE: &str = "major_dip";
const DEFAULT_TICK_SECS: u64 = 60;

pub async fn major_dip_candidate_loop(pool: PgPool) {
    info!("major_dip_candidate_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            continue;
        }
        match run_tick(&pool).await {
            Ok((scanned, written)) => {
                info!(scanned, written, "major_dip tick ok");
            }
            Err(e) => warn!(%e, "major_dip tick failed"),
        }
        let secs = load_tick_secs(&pool).await;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

// ─── config ───────────────────────────────────────────────────────────

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module = $1 AND config_key = 'enabled'",
    )
    .bind(MODULE)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return true; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

async fn load_tick_secs(pool: &PgPool) -> u64 {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module = $1 AND config_key = 'tick_secs'",
    )
    .bind(MODULE)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return DEFAULT_TICK_SECS; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_TICK_SECS)
        .max(15)
}

#[derive(Debug, Clone)]
struct Weights {
    structural: f64,
    fib_retrace: f64,
    volume_capit: f64,
    cvd_divergence: f64,
    indicator: f64,
    sentiment: f64,
    multi_tf: f64,
    funding_oi: f64,
    /// FAZ 25.4.A — Wyckoff phase + event alignment with the
    /// Elliott wave context. See docs/ELLIOTT_WYCKOFF_INTEGRATION.md
    /// for the full alignment matrix.
    wyckoff_alignment: f64,
}

impl Weights {
    fn defaults() -> Self {
        Self {
            structural: 0.18,
            fib_retrace: 0.13,
            volume_capit: 0.13,
            cvd_divergence: 0.08,
            indicator: 0.08,
            sentiment: 0.08,
            multi_tf: 0.08,
            funding_oi: 0.09,
            wyckoff_alignment: 0.15,
        }
    }
}

async fn load_weights(pool: &PgPool) -> Weights {
    let mut w = Weights::defaults();
    macro_rules! pull {
        ($field:ident, $key:expr) => {
            if let Ok(Some(row)) = sqlx::query(
                "SELECT value FROM system_config WHERE module=$1 AND config_key=$2",
            )
            .bind(MODULE)
            .bind($key)
            .fetch_optional(pool)
            .await
            {
                let val: Value = row.try_get("value").unwrap_or(Value::Null);
                if let Some(v) = val.get("value").and_then(|x| x.as_f64()) {
                    w.$field = v.clamp(0.0, 1.0);
                }
            }
        };
    }
    pull!(structural,        "weights.structural_completion");
    pull!(fib_retrace,       "weights.fib_retrace_quality");
    pull!(volume_capit,      "weights.volume_capitulation");
    pull!(cvd_divergence,    "weights.cvd_divergence");
    pull!(indicator,         "weights.indicator_alignment");
    pull!(sentiment,         "weights.sentiment_extreme");
    pull!(multi_tf,          "weights.multi_tf_confluence");
    pull!(funding_oi,        "weights.funding_oi_signals");
    pull!(wyckoff_alignment, "weights.wyckoff_alignment");
    w
}

// ─── tick ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct SymbolKey {
    exchange: String,
    segment: String,
    symbol: String,
    timeframe: String,
}

/// Polarity tag drives which extremum the worker hunts for and how
/// the volume / CVD / indicator / sentiment / funding scorers
/// interpret their inputs. FAZ 25.3.E adds Top alongside the
/// existing Dip; same composite formula, mirrored signal direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Polarity {
    Dip,
    Top,
}

impl Polarity {
    fn table(self) -> &'static str {
        match self {
            Polarity::Dip => "major_dip_candidates",
            Polarity::Top => "major_top_candidates",
        }
    }
}

async fn run_tick(pool: &PgPool) -> anyhow::Result<(usize, usize)> {
    let weights = load_weights(pool).await;
    let series = list_enabled_series(pool).await?;
    let mut scanned = 0usize;
    let mut written = 0usize;
    for s in &series {
        scanned += 1;
        // Run BOTH polarities back-to-back. Each writes to its own
        // table; if either fails the other still gets a chance.
        for polarity in [Polarity::Dip, Polarity::Top] {
            match score_series(pool, s, &weights, polarity).await {
                Ok(true) => written += 1,
                Ok(false) => {}
                Err(e) => warn!(
                    symbol=%s.symbol, tf=%s.timeframe, ?polarity, %e,
                    "major_pivot score failed"
                ),
            }
        }
    }
    Ok((scanned, written))
}

async fn list_enabled_series(pool: &PgPool) -> anyhow::Result<Vec<SymbolKey>> {
    let rows = sqlx::query(
        r#"SELECT DISTINCT exchange, segment, symbol, interval AS timeframe
             FROM engine_symbols WHERE enabled = true"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| SymbolKey {
            exchange: r.try_get("exchange").unwrap_or_default(),
            segment: r.try_get("segment").unwrap_or_default(),
            symbol: r.try_get("symbol").unwrap_or_default(),
            timeframe: r.try_get("timeframe").unwrap_or_default(),
        })
        .collect())
}

async fn score_series(
    pool: &PgPool,
    s: &SymbolKey,
    w: &Weights,
    polarity: Polarity,
) -> anyhow::Result<bool> {
    // 1) Find the candidate extremum: lowest pivot for Dip, highest
    //    for Top. Both use L2-prominence pivots so we focus on
    //    structural reversals, not micro-swings.
    let Some(extremum) = find_candidate_extremum(pool, s, polarity).await? else {
        return Ok(false);
    };

    // 2) Run each component scorer (polarity-aware where it matters).
    let c_struct = score_structural_completion(pool, s).await;
    let c_fib = score_fib_retrace(pool, s, &extremum, polarity).await;
    let c_volume = score_volume_capit(pool, s, &extremum, polarity).await;
    let c_cvd = score_cvd_divergence(pool, s, &extremum, polarity).await;
    let c_indicator = score_indicator_alignment(pool, s, polarity).await;
    let c_sentiment = score_sentiment_extreme(pool, s, polarity).await;
    let c_multi_tf = score_multi_tf_confluence(pool, s).await;
    let c_funding = score_funding_oi(pool, s, polarity).await;
    let (c_wyckoff, wyckoff_event_meta) =
        score_wyckoff_alignment(pool, s, polarity).await;

    let composite = w.structural * c_struct
        + w.fib_retrace * c_fib
        + w.volume_capit * c_volume
        + w.cvd_divergence * c_cvd
        + w.indicator * c_indicator
        + w.sentiment * c_sentiment
        + w.multi_tf * c_multi_tf
        + w.funding_oi * c_funding
        + w.wyckoff_alignment * c_wyckoff;
    let composite = composite.clamp(0.0, 1.0);

    let verdict = match composite {
        v if v < 0.30 => "low",
        v if v < 0.55 => "developing",
        v if v < 0.75 => "high",
        _ => "very_high",
    };

    let components = json!({
        "structural_completion": c_struct,
        "fib_retrace_quality":   c_fib,
        "volume_capitulation":   c_volume,
        "cvd_divergence":        c_cvd,
        "indicator_alignment":   c_indicator,
        "sentiment_extreme":     c_sentiment,
        "multi_tf_confluence":   c_multi_tf,
        "funding_oi_signals":    c_funding,
        "wyckoff_alignment":     c_wyckoff,
        "wyckoff_event":         wyckoff_event_meta,
    });

    let sql = format!(
        r#"INSERT INTO {}
              (exchange, segment, symbol, timeframe, candidate_bar,
               candidate_time, candidate_price, score, components, verdict)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
           ON CONFLICT (exchange, segment, symbol, timeframe, candidate_bar)
           DO UPDATE SET
               candidate_price = EXCLUDED.candidate_price,
               score           = EXCLUDED.score,
               components      = EXCLUDED.components,
               verdict         = EXCLUDED.verdict,
               updated_at      = now()"#,
        polarity.table()
    );
    sqlx::query(&sql)
        .bind(&s.exchange)
        .bind(&s.segment)
        .bind(&s.symbol)
        .bind(&s.timeframe)
        .bind(extremum.bar_index)
        .bind(extremum.time)
        .bind(extremum.price)
        .bind(composite)
        .bind(&components)
        .bind(verdict)
        .execute(pool)
        .await?;

    debug!(
        symbol=%s.symbol, tf=%s.timeframe, ?polarity, score=composite, verdict,
        "major_pivot score upserted"
    );
    Ok(true)
}

// ─── candidate dip lookup ─────────────────────────────────────────────

#[derive(Debug, Clone)]
struct DipPoint {
    bar_index: i64,
    time: DateTime<Utc>,
    price: rust_decimal::Decimal,
}

async fn find_candidate_extremum(
    pool: &PgPool,
    s: &SymbolKey,
    polarity: Polarity,
) -> anyhow::Result<Option<DipPoint>> {
    // L2 pivot in the last 200 bars; for Dip we want the lowest LOW
    // (direction=-1, ASC); for Top the highest HIGH (direction=+1,
    // DESC). Scoping to L2 keeps noise out (L0/L1 fire on every
    // micro-swing).
    let (pivot_dir, order_clause) = match polarity {
        Polarity::Dip => (-1_i16, "ASC"),
        Polarity::Top => (1_i16, "DESC"),
    };
    let sql = format!(
        r#"SELECT p.bar_index, p.open_time, p.price
             FROM pivots p
             JOIN engine_symbols es ON es.id = p.engine_symbol_id
            WHERE es.exchange = $1 AND es.segment = $2
              AND es.symbol = $3 AND es.interval = $4
              AND p.level = 2
              AND p.direction = $5
              AND p.bar_index >= COALESCE(
                  (SELECT MAX(bar_index) - 200
                     FROM pivots
                    WHERE engine_symbol_id = es.id),
                  0
              )
            ORDER BY p.price {}
            LIMIT 1"#,
        order_clause
    );
    let row = sqlx::query(&sql)
        .bind(&s.exchange)
        .bind(&s.segment)
        .bind(&s.symbol)
        .bind(&s.timeframe)
        .bind(pivot_dir)
        .fetch_optional(pool)
        .await?;
    let Some(r) = row else { return Ok(None); };
    Ok(Some(DipPoint {
        bar_index: r.try_get("bar_index").unwrap_or(0),
        time: r.try_get("open_time").unwrap_or_else(|_| Utc::now()),
        price: r.try_get("price").unwrap_or_default(),
    }))
}

// ─── component scorers ────────────────────────────────────────────────

/// 12.1 — Structural completion.
async fn score_structural_completion(pool: &PgPool, s: &SymbolKey) -> f64 {
    let row = sqlx::query(
        r#"SELECT current_wave, state, raw_meta
             FROM iq_structures
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4
              AND state IN ('candidate','tracking','completed')
            ORDER BY last_advanced_at DESC
            LIMIT 1"#,
    )
    .bind(&s.exchange).bind(&s.segment).bind(&s.symbol).bind(&s.timeframe)
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
                arr.iter()
                    .find(|x| x.get("kind").and_then(|k| k.as_str()) == Some(kind))
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

/// 12.2 — Fib retracement quality. Score peaks near {0.382, 0.500,
/// 0.618, 0.786} with linear decay.
async fn score_fib_retrace(
    pool: &PgPool,
    s: &SymbolKey,
    extremum: &DipPoint,
    _polarity: Polarity,
) -> f64 {
    let dip = extremum;
    let row = sqlx::query(
        r#"SELECT MAX(price) AS hi, MIN(price) AS lo
             FROM pivots p
             JOIN engine_symbols es ON es.id = p.engine_symbol_id
            WHERE es.exchange=$1 AND es.segment=$2
              AND es.symbol=$3 AND es.interval=$4
              AND p.level = 2
              AND p.bar_index BETWEEN $5 AND $6"#,
    )
    .bind(&s.exchange).bind(&s.segment).bind(&s.symbol).bind(&s.timeframe)
    .bind(dip.bar_index - 200).bind(dip.bar_index)
    .fetch_optional(pool).await.ok().flatten();
    let Some(r) = row else { return 0.0; };
    let hi: Option<rust_decimal::Decimal> = r.try_get("hi").ok();
    let lo: Option<rust_decimal::Decimal> = r.try_get("lo").ok();
    use rust_decimal::prelude::ToPrimitive;
    let hi_f = hi.and_then(|d| d.to_f64()).unwrap_or(0.0);
    let lo_f = lo.and_then(|d| d.to_f64()).unwrap_or(0.0);
    let dip_f = dip.price.to_f64().unwrap_or(0.0);
    if hi_f <= lo_f || hi_f - lo_f < 1e-9 {
        return 0.0;
    }
    let retrace_pct = (hi_f - dip_f) / (hi_f - lo_f);
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

/// 12.3 — Volume capitulation (Wyckoff SC heuristic).
async fn score_volume_capit(
    pool: &PgPool,
    s: &SymbolKey,
    extremum: &DipPoint,
    polarity: Polarity,
) -> f64 {
    let dip = extremum;
    // Pull last 20 bars around the dip + ATR.
    let rows = sqlx::query(
        r#"SELECT high, low, close, volume
             FROM market_bars
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND interval=$4
              AND open_time <= $5
            ORDER BY open_time DESC
            LIMIT 20"#,
    )
    .bind(&s.exchange).bind(&s.segment).bind(&s.symbol).bind(&s.timeframe).bind(dip.time)
    .fetch_all(pool).await.unwrap_or_default();
    if rows.len() < 14 {
        return 0.0;
    }
    use rust_decimal::prelude::ToPrimitive;
    let mut highs = Vec::with_capacity(rows.len());
    let mut lows = Vec::with_capacity(rows.len());
    let mut closes = Vec::with_capacity(rows.len());
    let mut vols = Vec::with_capacity(rows.len());
    for r in &rows {
        let h: rust_decimal::Decimal = r.try_get("high").unwrap_or_default();
        let l: rust_decimal::Decimal = r.try_get("low").unwrap_or_default();
        let c: rust_decimal::Decimal = r.try_get("close").unwrap_or_default();
        let v: rust_decimal::Decimal = r.try_get("volume").unwrap_or_default();
        highs.push(h.to_f64().unwrap_or(0.0));
        lows.push(l.to_f64().unwrap_or(0.0));
        closes.push(c.to_f64().unwrap_or(0.0));
        vols.push(v.to_f64().unwrap_or(0.0));
    }
    // Rough ATR(14) — average of high-low.
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
    // Find the bar with the climax volume to compute its range.
    let climax_idx = vols
        .iter()
        .position(|v| (*v - climax_vol).abs() < 1e-9)
        .unwrap_or(0);
    let climax_range = highs[climax_idx] - lows[climax_idx];
    let climax_range_atr = climax_range / atr;
    // Capitulation shadow direction depends on polarity:
    //   Dip → LONG LOWER shadow (sellers exhausted, rejection)
    //   Top → LONG UPPER shadow (buyers exhausted, blow-off top)
    let shadow_ratio = if climax_range > 1e-9 {
        match polarity {
            Polarity::Dip => (closes[climax_idx] - lows[climax_idx]) / climax_range,
            Polarity::Top => (highs[climax_idx] - closes[climax_idx]) / climax_range,
        }
    } else {
        0.0
    };
    let ratio_score = ((climax_ratio - 1.5) / 1.5).clamp(0.0, 1.0);
    let range_score = ((climax_range_atr - 1.0) / 1.0).clamp(0.0, 1.0);
    let shadow_score = ((shadow_ratio - 0.5).max(0.0)).min(0.5) * 2.0;
    0.4 * ratio_score + 0.3 * range_score + 0.3 * shadow_score
}

/// 12.4 — CVD bullish divergence (real impl, replaces stub).
///
/// Pulls last 60 closed bars + computes CVD via qtss-indicators::cvd.
/// Then looks for the classic bullish-regular-divergence signature:
/// price prints a new low (vs prior 30-bar low), but CVD prints a
/// HIGHER low at the same window. Magnitude scaled by how far CVD
/// rebounded from its prior low.
async fn score_cvd_divergence(
    pool: &PgPool,
    s: &SymbolKey,
    _extremum: &DipPoint,
    polarity: Polarity,
) -> f64 {
    let rows = sqlx::query(
        r#"SELECT high, low, close, volume FROM market_bars
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND interval=$4
            ORDER BY open_time DESC LIMIT 60"#,
    )
    .bind(&s.exchange).bind(&s.segment).bind(&s.symbol).bind(&s.timeframe)
    .fetch_all(pool).await.unwrap_or_default();
    if rows.len() < 30 {
        return 0.0;
    }
    use rust_decimal::prelude::ToPrimitive;
    // Newest first → reverse to chronological so CVD accumulates correctly.
    let mut highs = Vec::with_capacity(rows.len());
    let mut lows = Vec::with_capacity(rows.len());
    let mut closes = Vec::with_capacity(rows.len());
    let mut vols = Vec::with_capacity(rows.len());
    for r in rows.iter().rev() {
        let h: rust_decimal::Decimal = r.try_get("high").unwrap_or_default();
        let l: rust_decimal::Decimal = r.try_get("low").unwrap_or_default();
        let c: rust_decimal::Decimal = r.try_get("close").unwrap_or_default();
        let v: rust_decimal::Decimal = r.try_get("volume").unwrap_or_default();
        highs.push(h.to_f64().unwrap_or(0.0));
        lows.push(l.to_f64().unwrap_or(0.0));
        closes.push(c.to_f64().unwrap_or(0.0));
        vols.push(v.to_f64().unwrap_or(0.0));
    }
    let cvd = qtss_indicators::cvd::cvd(&highs, &lows, &closes, &vols);
    if cvd.len() < 30 {
        return 0.0;
    }
    // Two halves: older (first 30) and newer (last 30).
    let mid = cvd.len() / 2;
    let (older_p, older_c) = (&closes[..mid], &cvd[..mid]);
    let (newer_p, newer_c) = (&closes[mid..], &cvd[mid..]);
    // Polarity flip: Dip looks at LOWS (price LL + CVD HL = bullish);
    // Top looks at HIGHS (price HH + CVD LH = bearish).
    let pick_extremum = |slice: &[f64]| -> usize {
        match polarity {
            Polarity::Dip => slice
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0),
            Polarity::Top => slice
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0),
        }
    };
    let older_idx = pick_extremum(older_p);
    let newer_idx = pick_extremum(newer_p);
    let older_p_val = older_p[older_idx];
    let newer_p_val = newer_p[newer_idx];
    let older_cvd = older_c[older_idx];
    let newer_cvd = newer_c[newer_idx];
    let divergence = match polarity {
        // Dip: price newer-LOW + cvd HIGHER-low
        Polarity::Dip => newer_p_val < older_p_val && newer_cvd > older_cvd,
        // Top: price newer-HIGH + cvd LOWER-high
        Polarity::Top => newer_p_val > older_p_val && newer_cvd < older_cvd,
    };
    if divergence {
        let magnitude = (newer_cvd - older_cvd).abs() / older_cvd.abs().max(1.0);
        (magnitude / 0.5).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// 12.5 — RSI / MACD alignment (real impl, replaces stub).
///
/// Components:
///   rsi_div:           bullish-regular divergence between price low
///                      and RSI(14) low → 1.0 if present, else 0.0
///   macd_cross:        signal line cross AND histogram positive → 1.0
///   macd_div:          bullish-regular divergence on MACD histogram
///   rsi_oversold_reset: RSI was < 30 and now > 35 → momentum unclamping
async fn score_indicator_alignment(pool: &PgPool, s: &SymbolKey, polarity: Polarity) -> f64 {
    let rows = sqlx::query(
        r#"SELECT close FROM market_bars
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND interval=$4
            ORDER BY open_time DESC LIMIT 60"#,
    )
    .bind(&s.exchange).bind(&s.segment).bind(&s.symbol).bind(&s.timeframe)
    .fetch_all(pool).await.unwrap_or_default();
    if rows.len() < 30 {
        return 0.0;
    }
    use rust_decimal::prelude::ToPrimitive;
    let mut closes = Vec::with_capacity(rows.len());
    for r in rows.iter().rev() {
        let c: rust_decimal::Decimal = r.try_get("close").unwrap_or_default();
        closes.push(c.to_f64().unwrap_or(0.0));
    }
    let rsi = qtss_indicators::rsi::rsi(&closes, 14);
    let macd = qtss_indicators::macd::macd(&closes, 12, 26, 9);

    // Per-polarity helpers.
    let extremum_idx = |slice: &[f64]| -> usize {
        match polarity {
            Polarity::Dip => slice
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0),
            Polarity::Top => slice
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0),
        }
    };
    let price_outpaces = |new_p: f64, old_p: f64| match polarity {
        Polarity::Dip => new_p < old_p,
        Polarity::Top => new_p > old_p,
    };
    let momentum_diverges = |new_m: f64, old_m: f64| match polarity {
        Polarity::Dip => new_m > old_m,
        Polarity::Top => new_m < old_m,
    };

    // RSI divergence (regular, polarity-aware).
    let rsi_div = if closes.len() >= 30 && rsi.len() == closes.len() {
        let mid = closes.len() / 2;
        let oi = extremum_idx(&closes[..mid]);
        let ni_rel = extremum_idx(&closes[mid..]);
        let n_abs = mid + ni_rel;
        let rsi_o = rsi.get(oi).copied().unwrap_or(50.0);
        let rsi_n = rsi.get(n_abs).copied().unwrap_or(50.0);
        if price_outpaces(closes[n_abs], closes[oi])
            && momentum_diverges(rsi_n, rsi_o)
            && rsi_o.is_finite()
            && rsi_n.is_finite()
        {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    // MACD cross (Dip wants negative→positive flip; Top wants
    // positive→negative).
    let macd_cross = {
        let last_hist = macd.histogram.iter().rev().find(|x| x.is_finite()).copied().unwrap_or(0.0);
        let prev_hist = macd.histogram.iter().rev().filter(|x| x.is_finite()).nth(1).copied().unwrap_or(0.0);
        match polarity {
            Polarity::Dip if prev_hist <= 0.0 && last_hist > 0.0 => 1.0,
            Polarity::Top if prev_hist >= 0.0 && last_hist < 0.0 => 1.0,
            _ => 0.0,
        }
    };

    // MACD histogram divergence (regular, polarity-aware).
    let macd_div = if macd.histogram.len() >= 30 {
        let h = &macd.histogram;
        let mid = h.len() / 2;
        let oi = extremum_idx(&h[..mid]);
        let ni_rel = extremum_idx(&h[mid..]);
        let n_abs = mid + ni_rel;
        if price_outpaces(closes[n_abs], closes[oi])
            && momentum_diverges(h[n_abs], h[oi])
            && h[oi].is_finite()
            && h[n_abs].is_finite()
        {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    // Oversold/overbought reset.
    let last_rsi = rsi.iter().rev().find(|x| x.is_finite()).copied().unwrap_or(50.0);
    let reset = match polarity {
        Polarity::Dip => {
            let recent_min = rsi
                .iter()
                .rev()
                .take(20)
                .filter(|x| x.is_finite())
                .copied()
                .fold(f64::INFINITY, f64::min);
            if recent_min < 30.0 && last_rsi > 35.0 { 1.0 } else { 0.0 }
        }
        Polarity::Top => {
            let recent_max = rsi
                .iter()
                .rev()
                .take(20)
                .filter(|x| x.is_finite())
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            if recent_max > 70.0 && last_rsi < 65.0 { 1.0 } else { 0.0 }
        }
    };

    0.35 * rsi_div + 0.25 * macd_cross + 0.25 * macd_div + 0.15 * reset
}

/// 12.8 — Funding rate + OI clean reset (real impl, replaces stub).
///
/// Reads recent rows from `funding_rates` (qtss-derivatives-signals
/// canonical table) for the symbol. Sustained-negative funding +
/// declining OI alongside price drop = textbook short-overcrowding /
/// liquidation flush bottom signal.
async fn score_funding_oi(pool: &PgPool, s: &SymbolKey, polarity: Polarity) -> f64 {
    // 7-day average funding rate.
    let funding_avg: Option<f64> = sqlx::query_scalar::<_, Option<f64>>(
        r#"SELECT AVG(funding_rate)::DOUBLE PRECISION FROM funding_rates
            WHERE exchange=$1 AND symbol=$2
              AND captured_at > now() - interval '7 days'"#,
    )
    .bind(&s.exchange)
    .bind(&s.symbol)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten();
    // Dip: sustained NEGATIVE funding = shorts overcrowded → bottom.
    // Top: sustained POSITIVE funding = longs overcrowded → top.
    let funding_score = match (polarity, funding_avg) {
        (Polarity::Dip, Some(f)) if f <= -0.0005 => 1.0,
        (Polarity::Dip, Some(f)) if f <= -0.0002 => 0.5,
        (Polarity::Top, Some(f)) if f >= 0.0005 => 1.0,
        (Polarity::Top, Some(f)) if f >= 0.0002 => 0.5,
        _ => 0.0,
    };

    // OI 24h delta — relative to start of window.
    let oi_score: f64 = match sqlx::query(
        r#"SELECT
              (SELECT open_interest FROM open_interest_snapshots
                WHERE exchange=$1 AND symbol=$2
                ORDER BY captured_at DESC LIMIT 1) AS oi_now,
              (SELECT open_interest FROM open_interest_snapshots
                WHERE exchange=$1 AND symbol=$2
                  AND captured_at <= now() - interval '24 hours'
                ORDER BY captured_at DESC LIMIT 1) AS oi_24h_ago"#,
    )
    .bind(&s.exchange)
    .bind(&s.symbol)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(row)) => {
            let now: Option<rust_decimal::Decimal> = row.try_get("oi_now").ok();
            let prev: Option<rust_decimal::Decimal> = row.try_get("oi_24h_ago").ok();
            use rust_decimal::prelude::ToPrimitive;
            match (now.and_then(|d| d.to_f64()), prev.and_then(|d| d.to_f64())) {
                (Some(n), Some(p)) if p > 0.0 => {
                    let delta_pct = ((n - p) / p) * 100.0;
                    // Dip wants OI shrinking (longs liquidated, clean
                    // reset). Top wants OI shrinking too — but coupled
                    // with positive funding it signals long-side
                    // capitulation. Symmetric magnitude: both directions
                    // benefit from a >|10%| OI shift.
                    match polarity {
                        Polarity::Dip if delta_pct < -10.0 => 1.0,
                        Polarity::Dip if delta_pct < -5.0 => 0.5,
                        Polarity::Top if delta_pct < -10.0 => 1.0,
                        Polarity::Top if delta_pct < -5.0 => 0.5,
                        _ => 0.0,
                    }
                }
                _ => 0.0,
            }
        }
        _ => 0.0,
    };

    0.5 * funding_score + 0.5 * oi_score
}

/// FAZ 25.4.A — Wyckoff alignment with the active Elliott wave.
///
/// Reads:
///   1. iq_structures.current_wave for this (sym, tf)
///   2. latest detections row with pattern_family='wyckoff' for this
///      (sym, tf), inside `lookback` bars of the candidate dip
///
/// Returns (score, meta_json) where:
///   score ∈ 0..1 per the alignment matrix from
///         docs/ELLIOTT_WYCKOFF_INTEGRATION.md §II.1
///   meta_json carries the event subkind + phase + age so the
///         component breakdown can render "Spring + W2" etc. in
///         the GUI without an extra fetch.
async fn score_wyckoff_alignment(
    pool: &PgPool,
    s: &SymbolKey,
    polarity: Polarity,
) -> (f64, Value) {
    // Step 1 — current Elliott wave for this (sym, tf).
    let iq_row = sqlx::query(
        r#"SELECT current_wave FROM iq_structures
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4
              AND state IN ('candidate','tracking','completed')
            ORDER BY last_advanced_at DESC LIMIT 1"#,
    )
    .bind(&s.exchange).bind(&s.segment).bind(&s.symbol).bind(&s.timeframe)
    .fetch_optional(pool).await.ok().flatten();
    let current_wave: Option<String> = iq_row.and_then(|r| r.try_get("current_wave").ok());

    // Step 2 — latest Wyckoff event in the last 50 bars.
    let wy_row = sqlx::query(
        r#"SELECT subkind, raw_meta, end_time
             FROM detections
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4
              AND pattern_family='wyckoff' AND mode='live'
            ORDER BY end_time DESC LIMIT 1"#,
    )
    .bind(&s.exchange).bind(&s.segment).bind(&s.symbol).bind(&s.timeframe)
    .fetch_optional(pool).await.ok().flatten();
    let Some(row) = wy_row else {
        return (0.0, Value::Null);
    };
    let subkind: String = row.try_get("subkind").unwrap_or_default();
    let wy_meta: Value = row.try_get("raw_meta").unwrap_or(Value::Null);
    let phase: Option<String> = wy_meta
        .get("phase")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Step 3 — alignment matrix (research doc §II.1).
    // Polarity = Dip means we want bullish-reversal patterns; Top
    // wants bearish-reversal patterns. Wyckoff variants encoded in
    // subkind suffix (`_bull` / `_bear`) already match this lens.
    let cw = current_wave.as_deref().unwrap_or("");
    let alignment_score = match (polarity, subkind.as_str(), cw) {
        // ── Dip (bullish reversal lens) ─────────────────────────────
        // Spring + W2 = textbook accumulation Phase C → W3 launch
        (Polarity::Dip, "spring_bull", "W2") => 1.0,
        // Spring without W2 context — still a high-conviction event
        (Polarity::Dip, "spring_bull", _)    => 0.7,
        // SC = capitulation, end of a bearish move = next motive's W0
        (Polarity::Dip, "sc_bull", "C")      => 0.95,
        (Polarity::Dip, "sc_bull", _)        => 0.7,
        // SOS + W3 = Phase D markup launch confirmation
        (Polarity::Dip, "sos_bull", "W3")    => 0.85,
        (Polarity::Dip, "sos_bull", _)       => 0.5,
        // ST + W2 = secondary test of support during W2 retrace
        (Polarity::Dip, "st_bull", "W2")     => 0.7,
        (Polarity::Dip, "st_bull", _)        => 0.4,
        // LPS + W4 = higher-low after SOS coincides with W4 retrace
        (Polarity::Dip, "lps_bull", "W4")    => 0.7,
        (Polarity::Dip, "lps_bull", _)       => 0.4,
        // PS / Test / BU — supporting events, modest score
        (Polarity::Dip, "ps_bull", _)        => 0.4,
        (Polarity::Dip, "test_bull", _)      => 0.55,
        (Polarity::Dip, "bu_bull", _)        => 0.55,
        // AR — bounce after SC, marks range top, moderate dip signal
        (Polarity::Dip, "ar_bull", _)        => 0.4,

        // ── Top (bearish reversal lens) ─────────────────────────────
        // BC + W5 = blowoff distribution Phase A → A-wave launch
        (Polarity::Top, "bc_bear", "W5")     => 1.0,
        (Polarity::Top, "bc_bear", _)        => 0.7,
        // UTAD + B = Phase C distribution shakeout, C-wave imminent
        (Polarity::Top, "utad_bear", "B")    => 0.95,
        (Polarity::Top, "utad_bear", _)      => 0.7,
        // SOW + C = Phase D markdown
        (Polarity::Top, "sow_bear", "C")     => 0.85,
        (Polarity::Top, "sow_bear", _)       => 0.5,

        // Cross-polarity events (bullish event under top lens, etc.)
        // contribute ZERO — actively bearish for the lens.
        _ => 0.0,
    };

    let meta = json!({
        "subkind":       subkind,
        "phase":         phase,
        "current_wave":  current_wave,
        "score":         alignment_score,
    });
    (alignment_score, meta)
}

/// 12.6 — Sentiment extreme (Fear & Greed).
async fn score_sentiment_extreme(pool: &PgPool, _s: &SymbolKey, polarity: Polarity) -> f64 {
    // Try common table names — qtss-fearandgreed crate writes daily
    // snapshots. Falls back to 0.0 if the table or row is missing.
    let row = sqlx::query(
        r#"SELECT value FROM fear_greed_snapshots
            ORDER BY captured_at DESC LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await;
    let v: Option<f64> = match row {
        Ok(Some(r)) => r
            .try_get::<i32, _>("value")
            .ok()
            .map(|i| i as f64)
            .or_else(|| {
                r.try_get::<f64, _>("value").ok()
            }),
        _ => None,
    };
    let Some(v) = v else { return 0.0; };
    // Dip wants Extreme Fear (low F&G); Top wants Extreme Greed.
    match polarity {
        Polarity::Dip => {
            if v <= 25.0 { 1.0 }
            else if v <= 35.0 { 0.7 }
            else if v <= 45.0 { 0.3 }
            else { 0.0 }
        }
        Polarity::Top => {
            if v >= 75.0 { 1.0 }
            else if v >= 65.0 { 0.7 }
            else if v >= 55.0 { 0.3 }
            else { 0.0 }
        }
    }
}

/// 12.7 — Multi-TF confluence.
async fn score_multi_tf_confluence(pool: &PgPool, s: &SymbolKey) -> f64 {
    let parent_tf = match s.timeframe.as_str() {
        "15m" => "1h",
        "1h"  => "4h",
        "4h"  => "1d",
        "1d"  => "1w",
        _ => return 0.0,
    };
    let row = sqlx::query(
        r#"SELECT current_wave FROM iq_structures
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4
              AND state IN ('candidate','tracking')
            ORDER BY last_advanced_at DESC LIMIT 1"#,
    )
    .bind(&s.exchange).bind(&s.segment).bind(&s.symbol).bind(parent_tf)
    .fetch_optional(pool).await.ok().flatten();
    let Some(r) = row else { return 0.0; };
    let cw: String = r.try_get("current_wave").unwrap_or_default();
    match cw.as_str() {
        "W2" | "W4" | "C" => 1.0,
        "B" | "A" => 0.5,
        _ => 0.2,
    }
}
