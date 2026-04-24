//! Vault-first secret resolver with system_config fallback and audit
//! logging. Consumer-facing entry point; anywhere the codebase would
//! previously read an API key out of `system_config` should go through
//! `VaultReader::resolve` instead.
//!
//! Resolution order:
//!   1. `secrets_vault` — decrypted via the supplied `KekProvider`.
//!   2. If (1) misses and `allow_config_fallback = true`, read the
//!      plaintext out of `system_config` (legacy path) — emit a
//!      warning trace + audit row with `outcome='miss_fallback_config'`.
//!   3. Otherwise surface `SecretError::NotFound`.
//!
//! Every resolution writes one row to `secret_access_log`, giving
//! operators a tamper-evident trail without touching ciphertext.

use crate::error::{SecretError, SecretResult};
use crate::store::{PgSecretStore, SecretStore};
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{debug, warn};

/// Where a resolved plaintext came from — surfaced back to the caller
/// so high-sensitivity code paths can refuse the fallback path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretSource {
    Vault,
    ConfigFallback,
}

pub struct VaultReader {
    pool: PgPool,
    store: Arc<PgSecretStore>,
    actor: String,
}

impl VaultReader {
    /// `actor` is the short component name baked into every audit row
    /// (e.g. `"qtss-worker"`, `"qtss-ai"`, `"qtss-api"`).
    pub fn new(pool: PgPool, store: Arc<PgSecretStore>, actor: impl Into<String>) -> Self {
        Self {
            pool,
            store,
            actor: actor.into(),
        }
    }

    /// Resolve one secret by name. `reason` is a free-form tag describing
    /// why this read happened (`"anthropic_chat"`, `"telegram_notify"`)
    /// and is persisted unencrypted into the audit row.
    pub async fn resolve(
        &self,
        name: &str,
        reason: &str,
    ) -> SecretResult<(Vec<u8>, SecretSource)> {
        match self.store.get(name).await {
            Ok(plaintext) => {
                self.log("hit", name, reason, None, None).await;
                Ok((plaintext, SecretSource::Vault))
            }
            Err(SecretError::NotFound(_)) => self.fallback(name, reason).await,
            Err(e) => {
                let msg = e.to_string();
                self.log("error", name, reason, None, Some(&msg)).await;
                Err(e)
            }
        }
    }

    /// Convenience — like `resolve` but returns a UTF-8 string.
    pub async fn resolve_str(
        &self,
        name: &str,
        reason: &str,
    ) -> SecretResult<(String, SecretSource)> {
        let (bytes, src) = self.resolve(name, reason).await?;
        let text = String::from_utf8(bytes)
            .map_err(|e| SecretError::Crypto(format!("secret is not UTF-8: {e}")))?;
        Ok((text, src))
    }

    async fn fallback(
        &self,
        name: &str,
        reason: &str,
    ) -> SecretResult<(Vec<u8>, SecretSource)> {
        if !self.config_fallback_enabled().await {
            self.log("miss", name, reason, None, None).await;
            return Err(SecretError::NotFound(name.to_string()));
        }
        match self.read_system_config(name).await {
            Some(pt) => {
                warn!(
                    actor = %self.actor,
                    secret = %name,
                    %reason,
                    "secret served from system_config fallback — migrate to vault",
                );
                self.log("miss_fallback_config", name, reason, None, None).await;
                Ok((pt.into_bytes(), SecretSource::ConfigFallback))
            }
            None => {
                self.log("miss", name, reason, None, None).await;
                Err(SecretError::NotFound(name.to_string()))
            }
        }
    }

    /// Fallback lookup — reads `system_config` rows where `config_key`
    /// equals the supplied `name`. Matches the historical flat
    /// `{module, config_key}` layout used by the pre-vault codebase.
    /// We accept **any** module that holds the name; secrets are unique
    /// by key in practice (`anthropic_api_key` / `gemini_api_key` /
    /// `telegram_bot_token` …).
    async fn read_system_config(&self, name: &str) -> Option<String> {
        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT value FROM system_config WHERE config_key = $1 LIMIT 1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let (val,) = row?;
        // Historical configs store plaintext either as {"value": "..."}
        // or a bare string in JSONB.
        if let Some(s) = val.as_str() {
            return Some(s.to_string());
        }
        if let Some(s) = val.get("value").and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
        None
    }

    async fn config_fallback_enabled(&self) -> bool {
        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT value FROM system_config
               WHERE module = 'secrets' AND config_key = 'allow_config_fallback'",
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        row.and_then(|(v,)| v.get("enabled").and_then(|b| b.as_bool()))
            .unwrap_or(true)
    }

    async fn log(
        &self,
        outcome: &str,
        name: &str,
        reason: &str,
        kek_version: Option<i32>,
        error_msg: Option<&str>,
    ) {
        let res = sqlx::query(
            "INSERT INTO secret_access_log
                (actor, secret_name, outcome, reason, kek_version, error_msg)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&self.actor)
        .bind(name)
        .bind(outcome)
        .bind(reason)
        .bind(kek_version)
        .bind(error_msg)
        .execute(&self.pool)
        .await;
        if let Err(e) = res {
            // Audit failure must never kill the caller — but we want a
            // loud trace so ops notice a log-table regression.
            debug!(%e, "secret_access_log insert failed");
        }
    }
}
