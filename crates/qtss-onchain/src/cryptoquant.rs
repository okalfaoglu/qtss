//! CryptoQuant fetcher (paid, optional) — second source for the Chain
//! category alongside [`crate::glassnode`].
//!
//! Why two chain sources: Glassnode (Advanced $49/mo, yearly only) and
//! CryptoQuant (Advanced $39/mo, monthly + 7-day trial) cover roughly
//! the same metric set (SOPR, exchange netflow, MVRV) but with
//! different data freshness and pricing models. Operators can flip
//! one off and the other on via `system_config` without redeploying.
//! When both are enabled the aggregator naturally averages them since
//! they share `CategoryKind::Chain`.
//!
//! API surface (BTC; ETH path is symmetric):
//! - `/v1/btc/market-indicator/sopr`           → SOPR
//! - `/v1/btc/exchange-flows/netflow`          → exchange netflow
//! - `/v1/btc/market-indicator/mvrv`           → MVRV
//!
//! Auth: `Authorization: Bearer <api_key>` header.
//!
//! Like the Glassnode fetcher, the constructor returns `None` when no
//! key is configured so the worker can register it unconditionally
//! and let config gate it.

use async_trait::async_trait;
use serde::Deserialize;

use crate::types::{
    CategoryKind, CategoryReading, FetcherError, OnchainCategoryFetcher, OnchainDirection,
};

const BASE: &str = "https://api.cryptoquant.com/v1";

#[derive(Debug, Clone, Copy)]
pub struct CryptoQuantTuning {
    pub sopr_capitulation: f64,
    pub sopr_euphoria: f64,
    pub mvrv_undervalued: f64,
    pub mvrv_overheated: f64,
}

impl Default for CryptoQuantTuning {
    fn default() -> Self {
        // Same defaults as Glassnode — both APIs publish the same
        // canonical metric definition, so the thresholds carry over.
        Self {
            sopr_capitulation: 0.02,
            sopr_euphoria: 0.05,
            mvrv_undervalued: 1.0,
            mvrv_overheated: 3.5,
        }
    }
}

pub struct CryptoQuantFetcher {
    client: reqwest::Client,
    api_key: String,
    tuning: CryptoQuantTuning,
}

impl CryptoQuantFetcher {
    pub fn new(
        client: reqwest::Client,
        api_key: Option<String>,
        tuning: CryptoQuantTuning,
    ) -> Option<Self> {
        let key = api_key?.trim().to_string();
        if key.is_empty() {
            return None;
        }
        Some(Self { client, api_key: key, tuning })
    }
}

fn map_asset(symbol: &str) -> Result<&'static str, FetcherError> {
    let upper = symbol.to_ascii_uppercase();
    if upper.starts_with("BTC") {
        Ok("btc")
    } else if upper.starts_with("ETH") {
        Ok("eth")
    } else {
        Err(FetcherError::UnsupportedSymbol(symbol.to_string()))
    }
}

#[derive(Deserialize)]
struct CqEnvelope {
    result: Option<CqResult>,
}

#[derive(Deserialize)]
struct CqResult {
    data: Vec<CqPoint>,
}

/// CryptoQuant returns rows with metric-specific keys; we accept any
/// of the common ones via a serde rename alias chain.
#[derive(Deserialize)]
struct CqPoint {
    #[serde(alias = "value", alias = "sopr", alias = "mvrv", alias = "netflow_total")]
    v: Option<f64>,
}

async fn last_value(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
) -> Result<Option<f64>, FetcherError> {
    let env: CqEnvelope = client
        .get(format!("{BASE}{endpoint}"))
        .header("Authorization", format!("Bearer {api_key}"))
        .query(&[("window", "hour"), ("limit", "1")])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(env
        .result
        .and_then(|r| r.data.into_iter().next())
        .and_then(|p| p.v))
}

#[async_trait]
impl OnchainCategoryFetcher for CryptoQuantFetcher {
    fn name(&self) -> &'static str {
        "cryptoquant"
    }

    fn category(&self) -> CategoryKind {
        CategoryKind::Chain
    }

    fn cadence_s(&self) -> u64 {
        3600
    }

    async fn fetch(&self, symbol: &str) -> Result<CategoryReading, FetcherError> {
        let asset = map_asset(symbol)?;

        let sopr = last_value(
            &self.client,
            &format!("/{asset}/market-indicator/sopr"),
            &self.api_key,
        )
        .await
        .ok()
        .flatten();

        let netflow = last_value(
            &self.client,
            &format!("/{asset}/exchange-flows/netflow"),
            &self.api_key,
        )
        .await
        .ok()
        .flatten();

        let mvrv = last_value(
            &self.client,
            &format!("/{asset}/market-indicator/mvrv"),
            &self.api_key,
        )
        .await
        .ok()
        .flatten();

        let reading = blend(sopr, netflow, mvrv, self.tuning);
        // When all three API calls failed (confidence=0) propagate as
        // an error so the aggregator does not consume a phantom reading.
        if reading.confidence <= 0.0 {
            return Err(FetcherError::NoData(format!(
                "cryptoquant: all 3 endpoints returned no data for {symbol}"
            )));
        }
        Ok(reading)
    }
}

fn blend(
    sopr: Option<f64>,
    exchange_netflow: Option<f64>,
    mvrv: Option<f64>,
    t: CryptoQuantTuning,
) -> CategoryReading {
    let mut score = 0.0_f64;
    let mut details = Vec::new();
    let mut have = 0u32;

    if let Some(s) = sopr {
        have += 1;
        if s <= 1.0 - t.sopr_capitulation {
            score += 0.4;
            details.push(format!("[CQ] SOPR {s:.3} capitulation → bull"));
        } else if s >= 1.0 + t.sopr_euphoria {
            score -= 0.4;
            details.push(format!("[CQ] SOPR {s:.3} euphoria → bear"));
        }
    }

    if let Some(nf) = exchange_netflow {
        have += 1;
        if nf < 0.0 {
            score += 0.3;
            details.push("[CQ] exchange outflow (bullish)".into());
        } else if nf > 0.0 {
            score -= 0.3;
            details.push("[CQ] exchange inflow (bearish)".into());
        }
    }

    if let Some(m) = mvrv {
        have += 1;
        if m <= t.mvrv_undervalued {
            score += 0.3;
            details.push(format!("[CQ] MVRV {m:.2} undervalued → bull"));
        } else if m >= t.mvrv_overheated {
            score -= 0.3;
            details.push(format!("[CQ] MVRV {m:.2} overheated → bear"));
        }
    }

    let score = score.clamp(-1.0, 1.0);
    let confidence = match have {
        0 => 0.0,
        1 => 0.5,
        2 => 0.75,
        _ => 0.95,
    };
    let direction = if score > 0.05 {
        OnchainDirection::Long
    } else if score < -0.05 {
        OnchainDirection::Short
    } else {
        OnchainDirection::Neutral
    };

    CategoryReading {
        category: CategoryKind::Chain,
        score,
        confidence,
        direction: Some(direction),
        details,
        cadence_s: 3600, // window=hour
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitulation_signals_bull() {
        let r = blend(Some(0.95), Some(-1.0), Some(0.9), CryptoQuantTuning::default());
        assert!(r.score > 0.5);
        assert_eq!(r.direction, Some(OnchainDirection::Long));
    }

    #[test]
    fn euphoria_signals_bear() {
        let r = blend(Some(1.10), Some(1.0), Some(4.0), CryptoQuantTuning::default());
        assert!(r.score < -0.5);
    }

    #[test]
    fn missing_key_returns_none() {
        assert!(CryptoQuantFetcher::new(reqwest::Client::new(), None, CryptoQuantTuning::default()).is_none());
        assert!(CryptoQuantFetcher::new(reqwest::Client::new(), Some("".into()), CryptoQuantTuning::default()).is_none());
    }

    #[test]
    fn unsupported_symbol() {
        assert!(matches!(map_asset("DOGEUSDT"), Err(FetcherError::UnsupportedSymbol(_))));
        assert_eq!(map_asset("BTCUSDT").unwrap(), "btc");
        assert_eq!(map_asset("ETHUSDT").unwrap(), "eth");
    }
}
