//! Faz 9.3 — Model registry read API.
//!
//! The Python trainer writes rows into `qtss_models`. Rust only reads:
//!   * `list_models`   — full listing, newest first
//!   * `active_model`  — the one flagged active for a family (future
//!                       inference hook will call this).

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ModelRow {
    pub id: Uuid,
    pub model_family: String,
    pub model_version: String,
    pub feature_spec_version: i32,
    pub algorithm: String,
    pub task: String,
    pub n_train: i64,
    pub n_valid: i64,
    pub metrics_json: serde_json::Value,
    pub params_json: serde_json::Value,
    pub feature_names: Vec<String>,
    pub artifact_path: String,
    pub artifact_sha256: Option<String>,
    pub trained_at: DateTime<Utc>,
    pub trained_by: Option<String>,
    pub notes: Option<String>,
    pub active: bool,
    /// Faz 9B Kalem H — 'active' | 'shadow' | 'archived'. Source of truth
    /// for the GUI registry; `active` bool is kept in sync via a DB
    /// trigger so legacy readers keep working.
    pub role: String,
}

pub async fn list_models(
    pool: &PgPool,
    family: Option<&str>,
) -> Result<Vec<ModelRow>, StorageError> {
    let rows = match family {
        Some(f) => {
            sqlx::query_as::<_, ModelRow>(
                r#"
                SELECT id, model_family, model_version, feature_spec_version,
                       algorithm, task, n_train, n_valid,
                       metrics_json, params_json, feature_names,
                       artifact_path, artifact_sha256,
                       trained_at, trained_by, notes, active, role
                FROM qtss_models
                WHERE model_family = $1
                ORDER BY trained_at DESC
                "#,
            )
            .bind(f)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, ModelRow>(
                r#"
                SELECT id, model_family, model_version, feature_spec_version,
                       algorithm, task, n_train, n_valid,
                       metrics_json, params_json, feature_names,
                       artifact_path, artifact_sha256,
                       trained_at, trained_by, notes, active, role
                FROM qtss_models
                ORDER BY trained_at DESC
                "#,
            )
            .fetch_all(pool)
            .await?
        }
    };
    Ok(rows)
}

pub async fn active_model(
    pool: &PgPool,
    family: &str,
) -> Result<Option<ModelRow>, StorageError> {
    Ok(sqlx::query_as::<_, ModelRow>(
        r#"
        SELECT id, model_family, model_version, feature_spec_version,
               algorithm, task, n_train, n_valid,
               metrics_json, params_json, feature_names,
               artifact_path, artifact_sha256,
               trained_at, trained_by, notes, active
        FROM qtss_models
        WHERE model_family = $1 AND active = true
        LIMIT 1
        "#,
    )
    .bind(family)
    .fetch_optional(pool)
    .await?)
}

/// Flip active flag atomically: clear family, set (family,version) true.
pub async fn activate_model(
    pool: &PgPool,
    family: &str,
    version: &str,
) -> Result<(), StorageError> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE qtss_models SET active = false WHERE model_family = $1")
        .bind(family)
        .execute(&mut *tx)
        .await?;
    let res = sqlx::query(
        "UPDATE qtss_models SET active = true WHERE model_family = $1 AND model_version = $2",
    )
    .bind(family)
    .bind(version)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() != 1 {
        tx.rollback().await.ok();
        return Err(StorageError::Other(format!(
            "model {}/{} not found",
            family, version
        )));
    }
    tx.commit().await?;
    Ok(())
}

/// Faz 9B Kalem H — set `role` for a single (family, version). Values:
/// 'active' | 'shadow' | 'archived'. Validates against the CHECK
/// constraint up front so a bad role gets a readable error.
///
/// Demotion semantics: setting role='shadow' or 'archived' on the
/// currently-active row clears it (trigger maps role→active=false).
/// Promotion to 'active' requires the caller to also demote whatever
/// was active before; use `activate_model` for that — this function
/// does NOT displace the current active to keep the two paths
/// auditable separately.
pub async fn set_model_role(
    pool: &PgPool,
    family: &str,
    version: &str,
    role: &str,
) -> Result<(), StorageError> {
    const VALID: [&str; 3] = ["active", "shadow", "archived"];
    if !VALID.contains(&role) {
        return Err(StorageError::Other(format!(
            "invalid role '{role}' (must be active|shadow|archived)"
        )));
    }
    let res = sqlx::query(
        "UPDATE qtss_models SET role = $3 \
           WHERE model_family = $1 AND model_version = $2",
    )
    .bind(family)
    .bind(version)
    .bind(role)
    .execute(pool)
    .await?;
    if res.rows_affected() != 1 {
        return Err(StorageError::Other(format!(
            "model {family}/{version} not found"
        )));
    }
    Ok(())
}
