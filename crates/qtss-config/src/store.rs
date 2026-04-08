//! ConfigStore trait + Postgres-backed and in-memory implementations.
//!
//! Resolution algorithm (see `ResolveCtx::priority_chain`):
//! walk the priority chain top-to-bottom; first scope that has a value
//! for the requested key wins. If none of the override scopes have a value,
//! fall back to `config_schema.default_value`. If the key itself is unknown,
//! return `ConfigError::NotFound`.
//!
//! ## Why this layout (and not a giant match)
//! Following CLAUDE.md rule #1, the resolver does not branch per-scope.
//! It iterates a single ordered list (`priority_chain()`) and runs one
//! lookup per element. Adding a new scope type means adding it to
//! `ResolveCtx::priority_chain` — no other code changes.

use crate::error::{ConfigError, ConfigResult};
use crate::scope::{ResolveCtx, Scope, ScopeType};
use crate::types::{ConfigSchemaRow, ConfigValueRow, SetOptions};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::RwLock;
use uuid::Uuid;

#[async_trait]
pub trait ConfigStore: Send + Sync {
    /// Resolve a typed value for `key` using the priority chain in `ctx`.
    async fn get_json(&self, key: &str, ctx: &ResolveCtx) -> ConfigResult<serde_json::Value>;

    /// Convenience: resolve and deserialize into `T`.
    async fn get<T: DeserializeOwned>(&self, key: &str, ctx: &ResolveCtx) -> ConfigResult<T> {
        let value = self.get_json(key, ctx).await?;
        serde_json::from_value(value).map_err(ConfigError::Serde)
    }

    /// Insert or update a value at `scope` for `key`.
    /// `reason` is required (CLAUDE.md: every config change is auditable).
    async fn set_json(
        &self,
        key: &str,
        scope: &Scope,
        value: serde_json::Value,
        actor: Option<Uuid>,
        reason: &str,
        opts: SetOptions,
    ) -> ConfigResult<i32>;

    async fn set<T: Serialize + Send + Sync>(
        &self,
        key: &str,
        scope: &Scope,
        value: &T,
        actor: Option<Uuid>,
        reason: &str,
        opts: SetOptions,
    ) -> ConfigResult<i32> {
        let json = serde_json::to_value(value).map_err(ConfigError::Serde)?;
        self.set_json(key, scope, json, actor, reason, opts).await
    }

    /// Look up the schema row for `key`. Returns `NotFound` if absent.
    async fn schema(&self, key: &str) -> ConfigResult<ConfigSchemaRow>;
}

// ---------------------------------------------------------------------------
// PgConfigStore
// ---------------------------------------------------------------------------

pub struct PgConfigStore {
    pool: PgPool,
}

impl PgConfigStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Look up the scope_id for a `Scope`. Returns `ScopeNotFound` if absent
    /// — scopes must be pre-registered (seeded by migration or admin UI).
    async fn scope_id(&self, scope: &Scope) -> ConfigResult<i64> {
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM config_scope WHERE scope_type = $1 AND scope_key = $2",
        )
        .bind(scope.scope_type.as_str())
        .bind(&scope.key)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|(id,)| id).ok_or_else(|| ConfigError::ScopeNotFound {
            scope_type: scope.scope_type.as_str().to_string(),
            scope_key: scope.key.clone(),
        })
    }

    /// Probe a single (key, scope_id) and return the JSON value if present
    /// and currently valid (enabled + within valid_from/valid_until window).
    async fn probe(&self, key: &str, scope_id: i64) -> ConfigResult<Option<serde_json::Value>> {
        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT value FROM config_value
             WHERE key = $1
               AND scope_id = $2
               AND enabled = true
               AND (valid_from  IS NULL OR valid_from  <= now())
               AND (valid_until IS NULL OR valid_until >  now())",
        )
        .bind(key)
        .bind(scope_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(v,)| v))
    }

    async fn schema_default(&self, key: &str) -> ConfigResult<serde_json::Value> {
        let row: Option<(serde_json::Value,)> =
            sqlx::query_as("SELECT default_value FROM config_schema WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;

        row.map(|(v,)| v)
            .ok_or_else(|| ConfigError::NotFound(key.to_string()))
    }
}

#[async_trait]
impl ConfigStore for PgConfigStore {
    async fn get_json(&self, key: &str, ctx: &ResolveCtx) -> ConfigResult<serde_json::Value> {
        // Walk the priority chain. First hit wins. No if/else per scope type.
        for scope in ctx.priority_chain() {
            // A scope row may not exist (e.g. brand-new venue). Treat as miss.
            let scope_id = match self.scope_id(&scope).await {
                Ok(id) => id,
                Err(ConfigError::ScopeNotFound { .. }) => continue,
                Err(e) => return Err(e),
            };
            if let Some(value) = self.probe(key, scope_id).await? {
                return Ok(value);
            }
        }
        // Fallback: schema default. Errors with NotFound if the key itself
        // isn't registered — the only way to add a key is via config_schema.
        self.schema_default(key).await
    }

    async fn set_json(
        &self,
        key: &str,
        scope: &Scope,
        value: serde_json::Value,
        actor: Option<Uuid>,
        reason: &str,
        opts: SetOptions,
    ) -> ConfigResult<i32> {
        if reason.trim().is_empty() {
            return Err(ConfigError::MissingReason);
        }

        let scope_id = self.scope_id(scope).await?;
        let mut tx = self.pool.begin().await?;

        // Optimistic lock check + load current value for audit diff.
        let current: Option<(i32, serde_json::Value)> = sqlx::query_as(
            "SELECT version, value FROM config_value
             WHERE key = $1 AND scope_id = $2
             FOR UPDATE",
        )
        .bind(key)
        .bind(scope_id)
        .fetch_optional(&mut *tx)
        .await?;

        if let (Some(expected), Some((found, _))) = (opts.expected_version, &current) {
            if expected != *found {
                return Err(ConfigError::VersionConflict {
                    key: key.to_string(),
                    expected,
                    found: *found,
                });
            }
        }

        let (action, old_value, new_version) = match &current {
            Some((v, old)) => ("update", Some(old.clone()), v + 1),
            None => ("create", None, 1),
        };

        sqlx::query(
            "INSERT INTO config_value
                 (key, scope_id, value, version, enabled, valid_from, valid_until, updated_by, updated_at)
             VALUES ($1, $2, $3, $4, true, $5, $6, $7, now())
             ON CONFLICT (key, scope_id) DO UPDATE SET
                 value = EXCLUDED.value,
                 version = EXCLUDED.version,
                 valid_from = EXCLUDED.valid_from,
                 valid_until = EXCLUDED.valid_until,
                 updated_by = EXCLUDED.updated_by,
                 updated_at = now()",
        )
        .bind(key)
        .bind(scope_id)
        .bind(&value)
        .bind(new_version)
        .bind(opts.valid_from)
        .bind(opts.valid_until)
        .bind(actor)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO config_audit
                 (key, scope_id, action, old_value, new_value, changed_by, reason, correlation)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(key)
        .bind(scope_id)
        .bind(action)
        .bind(old_value)
        .bind(&value)
        .bind(actor)
        .bind(reason)
        .bind(opts.correlation)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(new_version)
    }

    async fn schema(&self, key: &str) -> ConfigResult<ConfigSchemaRow> {
        let row: Option<ConfigSchemaRow> = sqlx::query_as::<_, _>(
            "SELECT key, category, subcategory, value_type, json_schema, default_value,
                    unit, description, ui_widget, requires_restart, is_secret_ref, sensitivity,
                    deprecated_at, introduced_in, tags
             FROM config_schema
             WHERE key = $1",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;

        row.ok_or_else(|| ConfigError::NotFound(key.to_string()))
    }
}

// sqlx::FromRow for ConfigSchemaRow — manual impl to keep `types.rs` free of
// the sqlx dependency.
impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for ConfigSchemaRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            key: row.try_get("key")?,
            category: row.try_get("category")?,
            subcategory: row.try_get("subcategory")?,
            value_type: row.try_get("value_type")?,
            json_schema: row.try_get("json_schema")?,
            default_value: row.try_get("default_value")?,
            unit: row.try_get("unit")?,
            description: row.try_get("description")?,
            ui_widget: row.try_get("ui_widget")?,
            requires_restart: row.try_get("requires_restart")?,
            is_secret_ref: row.try_get("is_secret_ref")?,
            sensitivity: row.try_get("sensitivity")?,
            deprecated_at: row.try_get("deprecated_at")?,
            introduced_in: row.try_get("introduced_in")?,
            tags: row.try_get("tags")?,
        })
    }
}

// ---------------------------------------------------------------------------
// MemoryConfigStore — for unit tests of consumers and offline development.
// ---------------------------------------------------------------------------

/// In-memory implementation. Useful for unit tests of code that depends on
/// `ConfigStore` without standing up a database.
pub struct MemoryConfigStore {
    schemas: RwLock<HashMap<String, ConfigSchemaRow>>,
    values: RwLock<HashMap<(String, ScopeKey), serde_json::Value>>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ScopeKey {
    scope_type: ScopeType,
    key: String,
}

impl From<&Scope> for ScopeKey {
    fn from(s: &Scope) -> Self {
        Self {
            scope_type: s.scope_type,
            key: s.key.clone(),
        }
    }
}

impl Default for MemoryConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryConfigStore {
    pub fn new() -> Self {
        Self {
            schemas: RwLock::new(HashMap::new()),
            values: RwLock::new(HashMap::new()),
        }
    }

    pub fn register_schema(&self, schema: ConfigSchemaRow) {
        self.schemas
            .write()
            .expect("schemas lock poisoned")
            .insert(schema.key.clone(), schema);
    }

    /// Test helper — bypasses audit. Use only in tests / fixtures.
    pub fn put(&self, key: &str, scope: &Scope, value: serde_json::Value) {
        self.values
            .write()
            .expect("values lock poisoned")
            .insert((key.to_string(), ScopeKey::from(scope)), value);
    }
}

#[async_trait]
impl ConfigStore for MemoryConfigStore {
    async fn get_json(&self, key: &str, ctx: &ResolveCtx) -> ConfigResult<serde_json::Value> {
        let values = self.values.read().expect("values lock poisoned");
        for scope in ctx.priority_chain() {
            let lookup = (key.to_string(), ScopeKey::from(&scope));
            if let Some(v) = values.get(&lookup) {
                return Ok(v.clone());
            }
        }
        let schemas = self.schemas.read().expect("schemas lock poisoned");
        schemas
            .get(key)
            .map(|s| s.default_value.clone())
            .ok_or_else(|| ConfigError::NotFound(key.to_string()))
    }

    async fn set_json(
        &self,
        key: &str,
        scope: &Scope,
        value: serde_json::Value,
        _actor: Option<Uuid>,
        reason: &str,
        _opts: SetOptions,
    ) -> ConfigResult<i32> {
        if reason.trim().is_empty() {
            return Err(ConfigError::MissingReason);
        }
        // Schema must exist (mirrors PG FK behavior).
        if !self
            .schemas
            .read()
            .expect("schemas lock poisoned")
            .contains_key(key)
        {
            return Err(ConfigError::NotFound(key.to_string()));
        }
        self.values
            .write()
            .expect("values lock poisoned")
            .insert((key.to_string(), ScopeKey::from(scope)), value);
        Ok(1)
    }

    async fn schema(&self, key: &str) -> ConfigResult<ConfigSchemaRow> {
        self.schemas
            .read()
            .expect("schemas lock poisoned")
            .get(key)
            .cloned()
            .ok_or_else(|| ConfigError::NotFound(key.to_string()))
    }
}

// Silence unused-field warning until ConfigValueRow is consumed by PR-5 GUI loader.
#[allow(dead_code)]
fn _config_value_row_used(_r: ConfigValueRow) {}
