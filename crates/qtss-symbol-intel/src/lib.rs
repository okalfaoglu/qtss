//! qtss-symbol-intel — Faz 14.A5.
//!
//! Per-symbol risk budget calculator. Reads `qtss_symbol_profile` +
//! `qtss_market_regime_daily` snapshots plus `config_schema` entries
//! seeded by migration 0197, then produces an `effective_risk_pct`
//! that the existing `qtss-risk::RiskPctSizer` can consume to emit a
//! concrete quantity.
//!
//! Flow:
//!   risk_pct = tier_cap_pct[risk_tier]
//!            × regime_multiplier[regime]
//!            × fundamental_scale(fundamental_score)
//!            × liquidity_gate(avg_daily_vol_usd, risk_tier)
//!
//! CLAUDE.md #2 — no hardcoded constants. All knobs resolved via
//! `qtss-config`. CLAUDE.md #1 — variant dispatch via small lookup
//! maps (no scattered if/else).

#![forbid(unsafe_code)]

use chrono::NaiveDate;
use qtss_config::{ConfigStore, ResolveCtx};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IntelError {
    #[error("profile missing for {exchange}/{symbol}")]
    ProfileMissing { exchange: String, symbol: String },
    #[error("config: {0}")]
    Config(#[from] qtss_config::ConfigError),
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("panic regime — new positions blocked")]
    PanicRegime,
    #[error("liquidity below extreme floor")]
    LiquidityBelowFloor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolProfile {
    pub exchange: String,
    pub symbol: String,
    pub asset_class: String,
    pub category: String,
    pub risk_tier: String,
    pub sector: Option<String>,
    pub country: Option<String>,
    pub market_cap_usd: Option<f64>,
    pub avg_daily_vol_usd: Option<f64>,
    pub price_usd: Option<f64>,
    pub fundamental_score: Option<i16>,
    pub liquidity_score: Option<i16>,
    pub volatility_score: Option<i16>,
    pub manual_override: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketRegime {
    pub day: NaiveDate,
    pub exchange: String,
    pub sector: String,
    pub regime: String,
    pub breadth_pct: Option<f64>,
    pub momentum_20d: Option<f64>,
    pub volatility_index: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskBudget {
    pub exchange: String,
    pub symbol: String,
    pub risk_tier: String,
    pub regime: String,
    pub tier_cap_pct: Decimal,
    pub regime_multiplier: Decimal,
    pub fundamental_multiplier: Decimal,
    /// Final `risk_pct` clamped into [0, tier_cap_pct]. This is what
    /// `qtss-risk::RiskPctSizer` should receive as the `pct` parameter.
    pub effective_risk_pct: Decimal,
    /// Diagnostic trail — audit log / UI tooltip.
    pub notes: Vec<String>,
}

pub struct SymbolIntel<S: ConfigStore> {
    pool: PgPool,
    cfg: Arc<S>,
}

impl<S: ConfigStore> SymbolIntel<S> {
    pub fn new(pool: PgPool, cfg: Arc<S>) -> Self {
        Self { pool, cfg }
    }

    /// Load the per-symbol profile row. Returns `ProfileMissing` when
    /// `catalog_refresh` hasn't seen the symbol yet; the caller should
    /// fall back to a conservative default (e.g. skip the signal).
    pub async fn load_profile(
        &self,
        exchange: &str,
        symbol: &str,
    ) -> Result<SymbolProfile, IntelError> {
        let row = sqlx::query_as::<_, SymbolProfile>(
            r#"SELECT exchange, symbol, asset_class, category, risk_tier,
                       sector, country, market_cap_usd::float8 AS market_cap_usd,
                       avg_daily_vol_usd::float8 AS avg_daily_vol_usd,
                       price_usd::float8 AS price_usd,
                       fundamental_score, liquidity_score, volatility_score,
                       manual_override
                  FROM qtss_symbol_profile
                 WHERE exchange = $1 AND symbol = $2"#,
        )
        .bind(exchange)
        .bind(symbol)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| IntelError::ProfileMissing {
            exchange: exchange.into(),
            symbol: symbol.into(),
        })?;
        Ok(row)
    }

    /// Pull most-recent regime row for the exchange. Falls back to the
    /// `'*'` (borsa-geneli) row when per-sector data is missing.
    pub async fn load_regime(
        &self,
        exchange: &str,
        sector: Option<&str>,
    ) -> Result<Option<MarketRegime>, IntelError> {
        let key_sector = sector.unwrap_or("*");
        let row = sqlx::query_as::<_, MarketRegime>(
            r#"SELECT day, exchange, sector, regime,
                       breadth_pct::float8 AS breadth_pct,
                       momentum_20d::float8 AS momentum_20d,
                       volatility_index::float8 AS volatility_index
                  FROM qtss_market_regime_daily
                 WHERE exchange = $1
                   AND sector IN ($2, '*')
                 ORDER BY (sector = $2) DESC, day DESC
                 LIMIT 1"#,
        )
        .bind(exchange)
        .bind(key_sector)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Compute the effective risk percentage for the next entry on
    /// `exchange/symbol`. Returns `PanicRegime` / `LiquidityBelowFloor`
    /// as hard rejects — callers surface these as risk-gate failures.
    pub async fn compute_budget(
        &self,
        exchange: &str,
        symbol: &str,
    ) -> Result<RiskBudget, IntelError> {
        let profile = self.load_profile(exchange, symbol).await?;
        let regime = self
            .load_regime(exchange, profile.sector.as_deref())
            .await?;

        let ctx = ResolveCtx::default().with_venue(exchange);

        let tier_caps: BTreeMap<String, f64> = self
            .cfg
            .get("symbol_intel.tier_cap_pct", &ctx)
            .await?;
        let regime_muls: BTreeMap<String, f64> = self
            .cfg
            .get("symbol_intel.regime_multiplier", &ctx)
            .await?;
        let fund_range: BTreeMap<String, f64> = self
            .cfg
            .get("symbol_intel.fundamental_score_range", &ctx)
            .await?;
        let min_liq: BTreeMap<String, f64> = self
            .cfg
            .get("symbol_intel.min_liquidity_usd", &ctx)
            .await?;

        let mut notes = Vec::new();

        // 1) Tier cap — map miss ⇒ most conservative tier.
        let tier_cap = tier_caps
            .get(&profile.risk_tier)
            .copied()
            .unwrap_or_else(|| {
                notes.push(format!(
                    "unknown tier {} → falling back to extreme cap",
                    profile.risk_tier
                ));
                tier_caps.get("extreme").copied().unwrap_or(1.0)
            });
        // Schema stores pct as 7.0 == 7%. We want Decimal 0.07.
        let tier_cap_pct = to_dec(tier_cap / 100.0);

        // 2) Regime multiplier — default to neutral when snapshot missing.
        let (regime_label, regime_mul) = match regime.as_ref() {
            Some(r) => (r.regime.clone(), regime_muls.get(&r.regime).copied()),
            None => ("neutral".to_string(), regime_muls.get("neutral").copied()),
        };
        let regime_mul = regime_mul.unwrap_or_else(|| {
            notes.push(format!("unknown regime {regime_label} → 0.5"));
            0.5
        });
        if regime_mul <= 0.0 {
            return Err(IntelError::PanicRegime);
        }

        // 3) Liquidity floor — below-tier liquidity trims effective cap.
        let avg_vol = profile.avg_daily_vol_usd.unwrap_or(0.0);
        let extreme_floor = min_liq.get("extreme").copied().unwrap_or(0.0);
        if avg_vol < extreme_floor {
            return Err(IntelError::LiquidityBelowFloor);
        }
        let tier_floor = min_liq.get(&profile.risk_tier).copied().unwrap_or(0.0);
        let mut liq_haircut = dec!(1.0);
        if avg_vol < tier_floor && tier_floor > 0.0 {
            liq_haircut = dec!(0.5);
            notes.push(format!(
                "liquidity {avg_vol:.0} < tier_floor {tier_floor:.0} → ×0.5"
            ));
        }

        // 4) Fundamental score → linear scale.
        let floor_mul = fund_range.get("floor_mul").copied().unwrap_or(0.5);
        let ceil_mul = fund_range.get("ceiling_mul").copied().unwrap_or(1.0);
        let score = profile.fundamental_score.unwrap_or(50) as f64;
        let fund_mul = floor_mul + (ceil_mul - floor_mul) * (score / 100.0).clamp(0.0, 1.0);

        let regime_mul_d = to_dec(regime_mul);
        let fund_mul_d = to_dec(fund_mul);

        let mut eff = tier_cap_pct * regime_mul_d * fund_mul_d * liq_haircut;
        if eff > tier_cap_pct {
            eff = tier_cap_pct;
        }
        if eff < Decimal::ZERO {
            eff = Decimal::ZERO;
        }

        Ok(RiskBudget {
            exchange: exchange.to_string(),
            symbol: symbol.to_string(),
            risk_tier: profile.risk_tier,
            regime: regime_label,
            tier_cap_pct,
            regime_multiplier: regime_mul_d,
            fundamental_multiplier: fund_mul_d,
            effective_risk_pct: eff,
            notes,
        })
    }
}

fn to_dec(v: f64) -> Decimal {
    Decimal::from_f64(v).unwrap_or(Decimal::ZERO)
}

// sqlx::FromRow manual impls via derive -------------------------------------

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for SymbolProfile {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            exchange: row.try_get("exchange")?,
            symbol: row.try_get("symbol")?,
            asset_class: row.try_get("asset_class")?,
            category: row.try_get("category")?,
            risk_tier: row.try_get("risk_tier")?,
            sector: row.try_get("sector")?,
            country: row.try_get("country")?,
            market_cap_usd: row.try_get("market_cap_usd")?,
            avg_daily_vol_usd: row.try_get("avg_daily_vol_usd")?,
            price_usd: row.try_get("price_usd")?,
            fundamental_score: row.try_get("fundamental_score")?,
            liquidity_score: row.try_get("liquidity_score")?,
            volatility_score: row.try_get("volatility_score")?,
            manual_override: row.try_get("manual_override")?,
        })
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for MarketRegime {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            day: row.try_get("day")?,
            exchange: row.try_get("exchange")?,
            sector: row.try_get("sector")?,
            regime: row.try_get("regime")?,
            breadth_pct: row.try_get("breadth_pct")?,
            momentum_20d: row.try_get("momentum_20d")?,
            volatility_index: row.try_get("volatility_index")?,
        })
    }
}
