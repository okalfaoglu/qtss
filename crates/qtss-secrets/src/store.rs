//! Persistence for sealed secrets. PG store talks to `secrets_vault`;
//! memory store backs the unit tests so the cipher contract can be
//! exercised without a database.

use crate::cipher::{open, seal, SealedSecret};
use crate::error::{SecretError, SecretResult};
use crate::kek::KekProvider;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct StoredSecret {
    pub name: String,
    pub description: Option<String>,
    pub kek_version: i32,
    pub created_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn put(
        &self,
        name: &str,
        description: Option<&str>,
        plaintext: &[u8],
        actor: &str,
    ) -> SecretResult<StoredSecret>;

    async fn get(&self, name: &str) -> SecretResult<Vec<u8>>;

    async fn list(&self) -> SecretResult<Vec<StoredSecret>>;
}

// ---------------------------------------------------------------------------
// PgSecretStore
// ---------------------------------------------------------------------------

pub struct PgSecretStore {
    pool: PgPool,
    kek: Arc<dyn KekProvider>,
}

impl PgSecretStore {
    pub fn new(pool: PgPool, kek: Arc<dyn KekProvider>) -> Self {
        Self { pool, kek }
    }
}

#[async_trait]
impl SecretStore for PgSecretStore {
    async fn put(
        &self,
        name: &str,
        description: Option<&str>,
        plaintext: &[u8],
        actor: &str,
    ) -> SecretResult<StoredSecret> {
        let sealed = seal(self.kek.as_ref(), plaintext)?;
        let row: (DateTime<Utc>,) = sqlx::query_as(
            "INSERT INTO secrets_vault
                (name, description, wrapped_dek, ciphertext, nonce, kek_version, created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING created_at",
        )
        .bind(name)
        .bind(description)
        .bind(&sealed.wrapped_dek)
        .bind(&sealed.ciphertext)
        .bind(&sealed.nonce)
        .bind(sealed.kek_version)
        .bind(actor)
        .fetch_one(&self.pool)
        .await?;

        Ok(StoredSecret {
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            kek_version: sealed.kek_version,
            created_at: row.0,
            rotated_at: None,
        })
    }

    async fn get(&self, name: &str) -> SecretResult<Vec<u8>> {
        let row: Option<(Vec<u8>, Vec<u8>, Vec<u8>, i32)> = sqlx::query_as(
            "SELECT wrapped_dek, ciphertext, nonce, kek_version
             FROM secrets_vault WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        let (wrapped_dek, ciphertext, nonce, kek_version) =
            row.ok_or_else(|| SecretError::NotFound(name.to_string()))?;
        let sealed = SealedSecret {
            wrapped_dek,
            ciphertext,
            nonce,
            kek_version,
        };
        open(self.kek.as_ref(), &sealed)
    }

    async fn list(&self) -> SecretResult<Vec<StoredSecret>> {
        let rows: Vec<(
            String,
            Option<String>,
            i32,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
        )> = sqlx::query_as(
            "SELECT name, description, kek_version, created_at, rotated_at
             FROM secrets_vault ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| StoredSecret {
                name: r.0,
                description: r.1,
                kek_version: r.2,
                created_at: r.3,
                rotated_at: r.4,
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// MemorySecretStore — test-only
// ---------------------------------------------------------------------------

pub struct MemorySecretStore {
    kek: Arc<dyn KekProvider>,
    inner: Mutex<HashMap<String, (SealedSecret, StoredSecret)>>,
}

impl MemorySecretStore {
    pub fn new(kek: Arc<dyn KekProvider>) -> Self {
        Self {
            kek,
            inner: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl SecretStore for MemorySecretStore {
    async fn put(
        &self,
        name: &str,
        description: Option<&str>,
        plaintext: &[u8],
        _actor: &str,
    ) -> SecretResult<StoredSecret> {
        let sealed = seal(self.kek.as_ref(), plaintext)?;
        let meta = StoredSecret {
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            kek_version: sealed.kek_version,
            created_at: Utc::now(),
            rotated_at: None,
        };
        let mut guard = self.inner.lock().expect("memory secret store poisoned");
        if guard.contains_key(name) {
            return Err(SecretError::AlreadyExists(name.to_string()));
        }
        guard.insert(name.to_string(), (sealed, meta.clone()));
        Ok(meta)
    }

    async fn get(&self, name: &str) -> SecretResult<Vec<u8>> {
        let guard = self.inner.lock().expect("memory secret store poisoned");
        let (sealed, _) = guard
            .get(name)
            .ok_or_else(|| SecretError::NotFound(name.to_string()))?;
        open(self.kek.as_ref(), sealed)
    }

    async fn list(&self) -> SecretResult<Vec<StoredSecret>> {
        let guard = self.inner.lock().expect("memory secret store poisoned");
        let mut out: Vec<StoredSecret> = guard.values().map(|(_, m)| m.clone()).collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
}
