//! Shared OHLC bar type for range detectors.

use serde::{Deserialize, Serialize};

/// One OHLC candle with bar index and optional volume.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OhlcBar {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub bar_index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
}
