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
        .bind(self.cfg.window_minutes.to_string())
        .fetch_all(pool)
        .await?;

        let mut bull = 0.0f64;
        let mut bear = 0.0f64;
        let mut per_family: HashMap<String, (f64, usize)> = HashMap::new();

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
            match direction.signum() {
                1 => bull += contribution,
                -1 => bear += contribution,
                _ => {
                    // Neutral: split half each. This keeps neutral
                    // patterns (rectangles, doji) visible in both
                    // sides without forcing a direction.
                    bull += contribution * 0.5;
                    bear += contribution * 0.5;
                }
            }
            let entry = per_family.entry(family).or_insert((0.0, 0));
            entry.0 += contribution;
            entry.1 += 1;
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
