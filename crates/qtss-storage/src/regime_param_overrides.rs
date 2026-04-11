//! Storage for regime_param_overrides table (Faz 11).

use chrono::{DateTime, Utc};
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RegimeParamOverrideRow {
    pub id: Uuid,
    pub module: String,
    pub config_key: String,
    pub regime: String,
    pub value: Json<serde_json::Value>,
    pub description: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Get all overrides for a specific regime.
pub async fn list_overrides_for_regime(
    pool: &PgPool,
    regime: &str,
) -> Result<Vec<RegimeParamOverrideRow>, sqlx::Error> {
    sqlx::query_as::<_, RegimeParamOverrideRow>(
        r#"SELECT * FROM regime_param_overrides WHERE regime = $1 ORDER BY module, config_key"#,
    )
    .bind(regime)
    .fetch_all(pool)
    .await
}

/// Get all overrides (all regimes).
pub async fn list_all_overrides(
    pool: &PgPool,
) -> Result<Vec<RegimeParamOverrideRow>, sqlx::Error> {
    sqlx::query_as::<_, RegimeParamOverrideRow>(
        r#"SELECT * FROM regime_param_overrides ORDER BY module, config_key, regime"#,
    )
    .fetch_all(pool)
    .await
}

/// Get a specific override value.
pub async fn get_override(
    pool: &PgPool,
    module: &str,
    config_key: &str,
    regime: &str,
) -> Result<Option<RegimeParamOverrideRow>, sqlx::Error> {
    sqlx::query_as::<_, RegimeParamOverrideRow>(
        r#"SELECT * FROM regime_param_overrides
           WHERE module = $1 AND config_key = $2 AND regime = $3"#,
    )
    .bind(module)
    .bind(config_key)
    .bind(regime)
    .fetch_optional(pool)
    .await
}

/// Resolve an f64 override: returns the override value if present, else `default`.
pub async fn resolve_regime_f64(
    pool: &PgPool,
    module: &str,
    config_key: &str,
    regime: &str,
    default: f64,
) -> f64 {
    match get_override(pool, module, config_key, regime).await {
        Ok(Some(row)) => {
            if let Some(v) = row.value.as_f64() {
                v
            } else if let Some(s) = row.value.as_str() {
                s.parse().unwrap_or(default)
            } else {
                default
            }
        }
        _ => default,
    }
}

/// Upsert an override.
pub async fn upsert_override(
    pool: &PgPool,
    module: &str,
    config_key: &str,
    regime: &str,
    value: serde_json::Value,
    description: Option<&str>,
) -> Result<Uuid, sqlx::Error> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO regime_param_overrides (module, config_key, regime, value, description)
           VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (module, config_key, regime)
           DO UPDATE SET value = EXCLUDED.value, description = EXCLUDED.description, updated_at = now()
           RETURNING id"#,
    )
    .bind(module)
    .bind(config_key)
    .bind(regime)
    .bind(Json(value))
    .bind(description)
    .fetch_one(pool)
    .await?;
    Ok(id)
}
