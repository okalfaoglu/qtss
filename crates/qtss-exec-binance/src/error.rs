use thiserror::Error;

#[derive(Debug, Error)]
pub enum BinanceExecError {
    #[error("unsupported order type for binance spot: {0}")]
    UnsupportedOrderType(String),
    #[error("binance returned malformed response: {0}")]
    MalformedResponse(String),
    #[error("binance api error: {0}")]
    Api(String),
    #[error("fee model error: {0}")]
    Fees(String),
}

pub type BinanceExecResult<T> = Result<T, BinanceExecError>;
