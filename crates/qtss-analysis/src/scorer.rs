//! Confluence scorer — aggregates recent `detections` rows across all
//! pattern families into a single per-symbol-per-TF score.
//!
//! Scoring: score = Σ family_weight × structural_score × direction.
//! Direction is +1 / -1 / 0 so bullish detections and bearish
//! detections cancel out, giving a net bias. Separate `bull_score`
//! and `bear_score` accumulators expose the raw magnitudes in each
//! direction for strategy use.
//!
//! Regime-aware weighting: each family has an optional multiplier per
//! `RegimeKind`. E.g. harmonic patterns weighted down in `TrendingUp`
//! regime (mean-reversion setup loses edge in a strong trend).
//!
//! Config (CLAUDE.md #2 — all in `system_config`):
//!   * `confluence.weights.<family>`      → `{ "value": 1.0 }`
//!   * `confluence.regime.<family>.<regime>` → `{ "value": 1.0 }`
//!   * `confluence.window_minutes`         → `{ "value": 60 }`

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row};
use std::collections::HashMap;

/// One persisted confluence row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfluenceSnapshot {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub timeframe: String,
    pub bull_score: f64,
    pub bear_score: f64,
    pub net_score: f64,
    /// 0..1 normalised confidence derived from the raw net score.
    pub confidence: f64,
    pub verdict: ConfluenceVerdict,
    pub contributors: Value, // JSONB: {family: {weight, count, score}}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfluenceVerdict {
    StrongBull,
    WeakBull,
    Mixed,
    WeakBear,
    StrongBear,
}

impl ConfluenceVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StrongBull => "strong_bull",
            Self::WeakBull => "weak_bull",
            Self::Mixed => "mixed",
            Self::WeakBear => "weak_bear",
            Self::StrongBear => "strong_bear",
        }
    }
    pub fn from_score(score: f64, strong: f64) -> Self {
        if score >= strong {
            Self::StrongBull
        } else if score > 0.0 {
            Self::WeakBull
        } else if score <= -strong {
            Self::StrongBear
        } else if score < 0.0 {
            Self::WeakBear
        } else {
            Self::Mixed
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfluenceConfig {
    /// Lookback window (minutes) for detections considered fresh.
    pub window_minutes: i64,
    /// Score threshold for StrongBull / StrongBear verdict.
    pub strong_threshold: f64,
    /// Per-family base weight. Missing families default to 1.0.
    pub weights: HashMap<String, f64>,
    /// Per-family-per-regime multiplier. Key = "<family>:<regime>".
    pub regime_adjusters: HashMap<String, f64>,
}

impl Default for ConfluenceConfig {
    fn default() -> Self {
        let mut weights = HashMap::new();
        // Defaults favour structural patterns over raw candles.
        for (f, w) in [
            ("motive", 1.2),
            ("abc", 1.0),
            ("harmonic", 1.2),
            ("classical", 1.0),
            ("range", 0.9),
            ("gap", 0.8),
            ("candle", 0.3),
            ("orb", 0.8),
            ("smc", 1.1),
            ("derivatives", 0.9),
            ("orderflow", 0.9),
        ] {
            weights.insert(f.to_string(), w);
        }
        Self {
            window_minutes: 60,
            strong_threshold: 1.5,
            weights,
            regime_adjusters: HashMap::new(),
        }
    }
}

pub struct ConfluenceScorer {
    cfg: ConfluenceConfig,
}

impl ConfluenceScorer {
    pub fn new(cfg: ConfluenceConfig) -> Self {
        Self { cfg }
    }

    pub fn config(&self) -> &ConfluenceConfig {
        &self.cfg
    }

    /// Query the last `window_minutes` of detections for the given
    /// instrument-TF, compute the aggregate score, and return a
    /// snapshot. Optional `regime` biases the family weights by the
    /// `regime_adjusters` table.
    pub async fn compute(
        &self,
        pool: &PgPool,
        exchange: &str,
        segment: &str,
        symbol: &str,
        timeframe: &str,
        regime: Option<&str>,
    ) -> Result<ConfluenceSnapshot, sqlx::Error> {
        // TF-aware lookback — 60 min is plenty for 15m (4 bars of history)
        // but starves higher TFs. A 4h bar only closes every 240 min, so
        // with a 60-min window an instrument-TF can sit outside the
        // capture window and produce verdict=mixed forever. Scale the
        // base window by the bar length so every TF sees ≥4 bars of
        // upstream detection coverage.
        let window_minutes = tf_aware_window_minutes(timeframe, self.cfg.window_minutes);
        let rows = sqlx::query(
            r#"SELECT pattern_family,
                      direction,
                      COALESCE( (raw_meta->>'score')::float8, 0.6 ) AS score,
                      start_time
                 FROM detections
                WHERE exchange = $1
                  AND segment = $2
                  AND symbol = $3
                  AND (timeframe = $4 OR timeframe = '*')
                  AND invalidated = false
                  AND start_time >= now() - ($5 || ' minutes')::interval"#,
        )
        .bind(exchange)
        .bind(segment)
        .bind(symbol)
        .bind(timeframe)
        .bind(window_minutes.to_string())
        .fetch_all(pool)
        .await?;

        // v1.1.6 — per-family max (revert from v1.2.3 cluster
        // grouping). Cluster grouping over-collapsed the signal:
        // even on clean trend days the bull/bear imbalance shrank
        // below the strong threshold and the allocator went silent.
        //
        // Per-family max preserves the original intent (kill the
        // "3 SMC events = 3 votes" inflation) while letting
        // independent families (motive, harmonic, smc, wyckoff …)
        // each contribute one vote in their direction.
        let mut bull = 0.0f64;
        let mut bear = 0.0f64;
        let mut per_family: HashMap<String, (f64, usize)> = HashMap::new();
        // (family, direction_sign_key) -> best contribution seen so far.
        let mut family_best: HashMap<(String, &'static str), f64> = HashMap::new();

        for r in rows {
            let family: String = r.get("pattern_family");
            let direction: i16 = r.get("direction");
            let score_f: f64 = r.try_get("score").unwrap_or(0.6);
            let base_weight = self.cfg.weights.get(&family).copied().unwrap_or(1.0);
            let regime_key = regime
                .map(|r| format!("{family}:{r}"))
                .unwrap_or_else(|| format!("{family}:*"));
            let regime_mult = self
                .cfg
                .regime_adjusters
                .get(&regime_key)
                .copied()
                .unwrap_or(1.0);
            let contribution = base_weight * score_f.clamp(0.0, 1.0) * regime_mult;
            let dir_key: &'static str = match direction.signum() {
                1 => "b",
                -1 => "s",
                _ => "n",
            };
            let slot = family_best
                .entry((family.clone(), dir_key))
                .or_insert(0.0);
            if contribution > *slot {
                *slot = contribution;
            }
            let entry = per_family.entry(family).or_insert((0.0, 0));
            entry.0 += contribution;
            entry.1 += 1;
        }
        for ((_, dir_key), best) in family_best {
            match dir_key {
                "b" => bull += best,
                "s" => bear += best,
                _ => {
                    bull += best * 0.5;
                    bear += best * 0.5;
                }
            }
        }

        let net = bull - bear;
        // Normalize to 0..1 confidence using a logistic squash — avoids
        // the need to know the absolute maximum score ahead of time.
        let confidence = 1.0 / (1.0 + (-(net.abs()) / 1.5).exp());
        let contributors = serde_json::json!(per_family
            .into_iter()
            .map(|(k, (s, c))| (k, serde_json::json!({ "score": s, "count": c })))
            .collect::<HashMap<_, _>>());
        let verdict = ConfluenceVerdict::from_score(net, self.cfg.strong_threshold);

        Ok(ConfluenceSnapshot {
            exchange: exchange.to_string(),
            segment: segment.to_string(),
            symbol: symbol.to_string(),
            timeframe: timeframe.to_string(),
            bull_score: bull,
            bear_score: bear,
            net_score: net,
            confidence,
            verdict,
            contributors,
        })
    }
}

/// Load a `ConfluenceConfig` from `system_config`. Missing rows fall
/// back to the Rust defaults.
pub async fn load_config(pool: &PgPool) -> ConfluenceConfig {
    let mut cfg = ConfluenceConfig::default();
    // window
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'confluence' AND config_key = 'window_minutes'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(v) = val.get("value").and_then(|v| v.as_i64()) {
            cfg.window_minutes = v.max(1);
        }
    }
    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'confluence' AND config_key = 'strong_threshold'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(v) = val.get("value").and_then(|v| v.as_f64()) {
            cfg.strong_threshold = v;
        }
    }
    // weights.*
    let rows = sqlx::query(
        r#"SELECT config_key, value FROM system_config
            WHERE module = 'confluence' AND config_key LIKE 'weights.%'"#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for r in rows {
        let key: String = r.try_get("config_key").unwrap_or_default();
        let val: Value = r.try_get("value").unwrap_or(Value::Null);
        let Some(v) = val.get("value").and_then(|v| v.as_f64()) else { continue };
        let family = key.trim_start_matches("weights.").to_string();
        cfg.weights.insert(family, v);
    }
    // regime.<family>.<regime>
    let rows = sqlx::query(
        r#"SELECT config_key, value FROM system_config
            WHERE module = 'confluence' AND config_key LIKE 'regime.%'"#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for r in rows {
        let key: String = r.try_get("config_key").unwrap_or_default();
        let val: Value = r.try_get("value").unwrap_or(Value::Null);
        let Some(v) = val.get("value").and_then(|v| v.as_f64()) else { continue };
        let inner = key.trim_start_matches("regime.").to_string();
        cfg.regime_adjusters.insert(inner, v);
    }
    cfg
}

/// v1.2.3 — Map a pattern family into one of a handful of correlation
/// clusters. Within a cluster only the strongest contribution counts;
/// across clusters they sum. Tuned by hand from the families currently
/// produced by the engine writers — extend as new families come online.
fn family_to_cluster(family: &str) -> &'static str {
    match family {
        // Same underlying observation in different vocabularies — one
        // structural break tends to fire all three at once.
        "smc" | "classical" | "range" => "structure",
        // Wave-structure family.
        "motive" | "abc" => "elliott",
        // Composite-operator phase.
        "wyckoff" => "wyckoff",
        // Session-open context — gap + ORB usually share regime info.
        "gap" | "orb" => "context",
        // Tape / derivatives flow — both read order book + funding.
        "orderflow" | "derivatives" => "flow",
        // 1-3 bar reversal — kept solo because it is intentionally
        // weak and shouldn't piggy-back on structure clusters.
        "candle" => "candle",
        _ => "solo",
    }
}

/// Translate a symbolic timeframe into minutes per bar. Unknown strings
/// fall back to 60 so the default-60-minute lookback continues to work
/// as before rather than silently amplifying to zero.
fn tf_bar_minutes(tf: &str) -> i64 {
    match tf {
        "1m" => 1,
        "3m" => 3,
        "5m" => 5,
        "15m" => 15,
        "30m" => 30,
        "1h" => 60,
        "2h" => 120,
        "4h" => 240,
        "6h" => 360,
        "8h" => 480,
        "12h" => 720,
        "1d" => 1440,
        "3d" => 4320,
        "1w" => 10080,
        _ => 60,
    }
}

/// Stretch the configured confluence window so that higher timeframes
/// still see ≥4 bars of history. `base` (the system_config
/// `confluence.window_minutes`) acts as the floor for the default 15m
/// path; anything larger wins. Capped at two weeks so a misconfigured
/// TF doesn't accidentally scan the whole detection table.
fn tf_aware_window_minutes(timeframe: &str, base: i64) -> i64 {
    let bar = tf_bar_minutes(timeframe);
    let scaled = bar * 4;
    base.max(scaled).min(14 * 24 * 60) // cap at 2 weeks
}

#[cfg(test)]
mod tf_window_tests {
    use super::*;

    #[test]
    fn low_tf_uses_base() {
        // 15m × 4 = 60min — ties go to base, so 60 wins.
        assert_eq!(tf_aware_window_minutes("15m", 60), 60);
        // base 120 still wins over low-TF scale.
        assert_eq!(tf_aware_window_minutes("15m", 120), 120);
    }

    #[test]
    fn high_tf_scales_past_base() {
        assert_eq!(tf_aware_window_minutes("4h", 60), 240 * 4); // 16h window
        assert_eq!(tf_aware_window_minutes("1d", 60), 1440 * 4); // 4 day window
    }

    #[test]
    fn cap_holds() {
        // Absurd TF still clamps to 2-week ceiling.
        let v = tf_aware_window_minutes("1w", 60);
        assert!(v <= 14 * 24 * 60);
    }

    #[test]
    fn unknown_tf_defaults() {
        // Unknown → 60 min per bar, × 4 bars = 240, wins over base 60.
        assert_eq!(tf_aware_window_minutes("bogus", 60), 240);
    }
}
