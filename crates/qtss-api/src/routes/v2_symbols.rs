//! `GET /v2/symbols` — Faz 14.A8.
//!
//! Read-only listing of `qtss_symbol_profile` rows for the operator
//! GUI. Filters by exchange / tier / category and free-text symbol.
//! Paired with `POST /v2/symbols/:exchange/:symbol/override` so
//! operators can toggle `manual_override` + edit tier from the page.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use qtss_config::PgConfigStore;
use qtss_symbol_intel::{IntelError, SymbolIntel};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct SymbolRow {
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
    pub notes: Option<String>,
    pub source: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct SymbolFeed {
    pub generated_at: DateTime<Utc>,
    pub total: i64,
    pub entries: Vec<SymbolRow>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub exchange: Option<String>,
    pub tier: Option<String>,
    pub category: Option<String>,
    pub q: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct OverridePatch {
    pub manual_override: Option<bool>,
    pub risk_tier: Option<String>,
    pub category: Option<String>,
    pub notes: Option<String>,
}

pub fn v2_symbols_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/symbols", get(list_symbols))
        .route(
            "/v2/symbols/{exchange}/{symbol}/override",
            post(patch_override),
        )
        .route(
            "/v2/symbols/{exchange}/{symbol}/budget",
            get(get_budget),
        )
}

#[derive(Debug, Serialize)]
pub struct BudgetResponse {
    pub exchange: String,
    pub symbol: String,
    pub risk_tier: String,
    pub regime: String,
    pub tier_cap_pct: Decimal,
    pub regime_multiplier: Decimal,
    pub fundamental_multiplier: Decimal,
    pub effective_risk_pct: Decimal,
    pub notes: Vec<String>,
    /// Hard-reject status when compute_budget refuses to size a position.
    /// `None` → normal budget; `Some("panic_regime"|"liquidity_floor"|"profile_missing")`.
    pub blocked: Option<String>,
}

async fn get_budget(
    State(st): State<SharedState>,
    Path((exchange, symbol)): Path<(String, String)>,
) -> Result<Json<BudgetResponse>, ApiError> {
    let intel = SymbolIntel::new(
        st.pool.clone(),
        Arc::new(PgConfigStore::new(st.pool.clone())),
    );
    match intel.compute_budget(&exchange, &symbol).await {
        Ok(b) => Ok(Json(BudgetResponse {
            exchange: b.exchange,
            symbol: b.symbol,
            risk_tier: b.risk_tier,
            regime: b.regime,
            tier_cap_pct: b.tier_cap_pct,
            regime_multiplier: b.regime_multiplier,
            fundamental_multiplier: b.fundamental_multiplier,
            effective_risk_pct: b.effective_risk_pct,
            notes: b.notes,
            blocked: None,
        })),
        Err(IntelError::PanicRegime) => Ok(Json(blocked(&exchange, &symbol, "panic_regime"))),
        Err(IntelError::LiquidityBelowFloor) => {
            Ok(Json(blocked(&exchange, &symbol, "liquidity_floor")))
        }
        Err(IntelError::ProfileMissing { .. }) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "symbol not in qtss_symbol_profile — run catalog refresh",
        )),
        Err(e) => Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
        )),
    }
}

fn blocked(exchange: &str, symbol: &str, reason: &str) -> BudgetResponse {
    BudgetResponse {
        exchange: exchange.to_string(),
        symbol: symbol.to_string(),
        risk_tier: String::new(),
        regime: String::new(),
        tier_cap_pct: Decimal::ZERO,
        regime_multiplier: Decimal::ZERO,
        fundamental_multiplier: Decimal::ZERO,
        effective_risk_pct: Decimal::ZERO,
        notes: vec![format!("blocked: {reason}")],
        blocked: Some(reason.to_string()),
    }
}

async fn list_symbols(
    State(st): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<SymbolFeed>, ApiError> {
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    let like = q.q.as_deref().map(|s| format!("%{}%", s.to_uppercase()));

    let rows = sqlx::query(
        r#"
        SELECT exchange, symbol, asset_class, category, risk_tier,
               sector, country,
               market_cap_usd::float8      AS market_cap_usd,
               avg_daily_vol_usd::float8   AS avg_daily_vol_usd,
               price_usd::float8           AS price_usd,
               fundamental_score, liquidity_score, volatility_score,
               manual_override, notes, source, updated_at
          FROM qtss_symbol_profile
         WHERE ($1::text IS NULL OR exchange = $1)
           AND ($2::text IS NULL OR risk_tier = $2)
           AND ($3::text IS NULL OR category = $3)
           AND ($4::text IS NULL OR symbol ILIKE $4)
         ORDER BY COALESCE(market_cap_usd, 0) DESC, symbol
         LIMIT $5
        "#,
    )
    .bind(q.exchange.as_deref())
    .bind(q.tier.as_deref())
    .bind(q.category.as_deref())
    .bind(like.as_deref())
    .bind(limit)
    .fetch_all(&st.pool)
    .await?;

    let entries: Vec<SymbolRow> = rows
        .into_iter()
        .map(|r| SymbolRow {
            exchange: r.get("exchange"),
            symbol: r.get("symbol"),
            asset_class: r.get("asset_class"),
            category: r.get("category"),
            risk_tier: r.get("risk_tier"),
            sector: r.get("sector"),
            country: r.get("country"),
            market_cap_usd: r.get("market_cap_usd"),
            avg_daily_vol_usd: r.get("avg_daily_vol_usd"),
            price_usd: r.get("price_usd"),
            fundamental_score: r.get("fundamental_score"),
            liquidity_score: r.get("liquidity_score"),
            volatility_score: r.get("volatility_score"),
            manual_override: r.get("manual_override"),
            notes: r.get("notes"),
            source: r.get("source"),
            updated_at: r.get("updated_at"),
        })
        .collect();

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM qtss_symbol_profile")
        .fetch_one(&st.pool)
        .await?;

    Ok(Json(SymbolFeed {
        generated_at: Utc::now(),
        total,
        entries,
    }))
}

async fn patch_override(
    State(st): State<SharedState>,
    Path((exchange, symbol)): Path<(String, String)>,
    Json(p): Json<OverridePatch>,
) -> Result<Json<SymbolRow>, ApiError> {
    sqlx::query(
        r#"UPDATE qtss_symbol_profile
              SET manual_override = COALESCE($3, manual_override),
                  risk_tier       = COALESCE($4, risk_tier),
                  category        = COALESCE($5, category),
                  notes           = COALESCE($6, notes),
                  updated_at      = now()
            WHERE exchange = $1 AND symbol = $2"#,
    )
    .bind(&exchange)
    .bind(&symbol)
    .bind(p.manual_override)
    .bind(p.risk_tier.as_deref())
    .bind(p.category.as_deref())
    .bind(p.notes.as_deref())
    .execute(&st.pool)
    .await?;

    let row = sqlx::query(
        r#"SELECT exchange, symbol, asset_class, category, risk_tier,
                   sector, country,
                   market_cap_usd::float8 AS market_cap_usd,
                   avg_daily_vol_usd::float8 AS avg_daily_vol_usd,
                   price_usd::float8 AS price_usd,
                   fundamental_score, liquidity_score, volatility_score,
                   manual_override, notes, source, updated_at
              FROM qtss_symbol_profile
             WHERE exchange = $1 AND symbol = $2"#,
    )
    .bind(&exchange)
    .bind(&symbol)
    .fetch_one(&st.pool)
    .await?;

    Ok(Json(SymbolRow {
        exchange: row.get("exchange"),
        symbol: row.get("symbol"),
        asset_class: row.get("asset_class"),
        category: row.get("category"),
        risk_tier: row.get("risk_tier"),
        sector: row.get("sector"),
        country: row.get("country"),
        market_cap_usd: row.get("market_cap_usd"),
        avg_daily_vol_usd: row.get("avg_daily_vol_usd"),
        price_usd: row.get("price_usd"),
        fundamental_score: row.get("fundamental_score"),
        liquidity_score: row.get("liquidity_score"),
        volatility_score: row.get("volatility_score"),
        manual_override: row.get("manual_override"),
        notes: row.get("notes"),
        source: row.get("source"),
        updated_at: row.get("updated_at"),
    }))
}
