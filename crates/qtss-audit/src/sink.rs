//! Audit sinks. The PG sink is the production target; the in-memory sink
//! is for unit tests so the chain logic can be exercised without a DB.

use crate::chain::{canonical_payload, hash_row};
use crate::error::AuditResult;
use crate::types::{AuditRecord, NewAuditEvent};
use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;
use std::sync::{Arc, Mutex};

#[async_trait]
pub trait AuditSink: Send + Sync {
    /// Append an event. Implementations are responsible for computing
    /// the hash chain link relative to the most recent existing row.
    async fn append(&self, evt: NewAuditEvent) -> AuditResult<AuditRecord>;

    /// Read all rows in insertion order. Used by the verifier.
    async fn read_all(&self) -> AuditResult<Vec<AuditRecord>>;
}

// ---------------------------------------------------------------------------
// PgAuditSink
// ---------------------------------------------------------------------------

pub struct PgAuditSink {
    pool: PgPool,
}

impl PgAuditSink {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AuditSink for PgAuditSink {
    async fn append(&self, evt: NewAuditEvent) -> AuditResult<AuditRecord> {
        // Run inside a transaction so the prev_hash lookup and the insert
        // are atomic — two concurrent writers can't both compute the chain
        // off the same tip and produce a fork.
        let mut tx = self.pool.begin().await?;

        let prev_hash: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT row_hash FROM qtss_audit_log ORDER BY id DESC LIMIT 1 FOR UPDATE",
        )
        .fetch_optional(&mut *tx)
        .await?;

        let canonical = canonical_payload(&evt)?;
        let row_hash = hash_row(prev_hash.as_deref(), &canonical);

        let row: (i64, chrono::DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO qtss_audit_log
                (actor, action, subject, payload, correlation_id, prev_hash, row_hash)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id, at",
        )
        .bind(&evt.actor)
        .bind(&evt.action)
        .bind(&evt.subject)
        .bind(&evt.payload)
        .bind(evt.correlation_id)
        .bind(prev_hash.as_deref())
        .bind(&row_hash)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(AuditRecord {
            id: row.0,
            at: row.1,
            actor: evt.actor,
            action: evt.action,
            subject: evt.subject,
            payload: evt.payload,
            correlation_id: evt.correlation_id,
            prev_hash,
            row_hash,
        })
    }

    async fn read_all(&self) -> AuditResult<Vec<AuditRecord>> {
        let rows: Vec<(
            i64,
            chrono::DateTime<Utc>,
            String,
            String,
            String,
            serde_json::Value,
            Option<uuid::Uuid>,
            Option<Vec<u8>>,
            Vec<u8>,
        )> = sqlx::query_as(
            "SELECT id, at, actor, action, subject, payload, correlation_id, prev_hash, row_hash
             FROM qtss_audit_log ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| AuditRecord {
                id: r.0,
                at: r.1,
                actor: r.2,
                action: r.3,
                subject: r.4,
                payload: r.5,
                correlation_id: r.6,
                prev_hash: r.7,
                row_hash: r.8,
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// MemoryAuditSink — test-only
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MemoryAuditSink {
    inner: Arc<Mutex<Vec<AuditRecord>>>,
}

impl MemoryAuditSink {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AuditSink for MemoryAuditSink {
    async fn append(&self, evt: NewAuditEvent) -> AuditResult<AuditRecord> {
        let mut guard = self.inner.lock().expect("memory audit sink poisoned");
        let prev_hash = guard.last().map(|r| r.row_hash.clone());
        let canonical = canonical_payload(&evt)?;
        let row_hash = hash_row(prev_hash.as_deref(), &canonical);
        let id = guard.len() as i64 + 1;
        let rec = AuditRecord {
            id,
            at: Utc::now(),
            actor: evt.actor,
            action: evt.action,
            subject: evt.subject,
            payload: evt.payload,
            correlation_id: evt.correlation_id,
            prev_hash,
            row_hash,
        };
        guard.push(rec.clone());
        Ok(rec)
    }

    async fn read_all(&self) -> AuditResult<Vec<AuditRecord>> {
        Ok(self
            .inner
            .lock()
            .expect("memory audit sink poisoned")
            .clone())
    }
}
