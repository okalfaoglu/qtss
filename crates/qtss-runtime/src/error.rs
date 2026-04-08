use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("operation '{op}' not allowed in {mode:?} mode")]
    NotAllowed { op: &'static str, mode: crate::RunMode },
    #[error("invalid runtime config: {0}")]
    InvalidConfig(String),
}

pub type RuntimeResult<T> = Result<T, RuntimeError>;
