//! Typed errors for `qtss-analysis` public surfaces (traits, loop helpers).

use thiserror::Error;

/// Confluence / snapshot persistence failures surfaced from [`crate::ConfluencePersist`].
#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error(transparent)]
    Storage(#[from] qtss_storage::StorageError),
}
