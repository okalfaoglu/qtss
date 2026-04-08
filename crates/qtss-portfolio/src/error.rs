use thiserror::Error;

#[derive(Debug, Error)]
pub enum PortfolioError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("unknown instrument: {0}")]
    UnknownInstrument(String),
}

pub type PortfolioResult<T> = Result<T, PortfolioError>;
