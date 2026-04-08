use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("audit chain broken at row id={id}: expected prev_hash={expected}, found {found}")]
    ChainBroken {
        id: i64,
        expected: String,
        found: String,
    },
    #[error("row hash mismatch at row id={id}: stored={stored}, recomputed={recomputed}")]
    HashMismatch {
        id: i64,
        stored: String,
        recomputed: String,
    },
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub type AuditResult<T> = Result<T, AuditError>;
