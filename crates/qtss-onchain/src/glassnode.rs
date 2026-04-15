//! Glassnode fetcher (paid, optional).
//!
//! Endpoints used (Advanced tier, $30/mo):
//! - `/v1/metrics/indicators/sopr` — Spent Output Profit Ratio.
//!   <1 = realised losses (capitulation, bullish at extremes).
//! - `/v1/metrics/transactions/transfers_volume_to_exchanges_sum`
//!   netflow direction.
//! - `/v1/metrics/market/mvrv` — MVRV; <1 undervalued, >3.7 overheated.
//!
//! Asset support is limited to BTC/ETH for v1 — anything else returns
//! [`FetcherError::UnsupportedSymbol`] and the aggregator skips it.
//!
//! When the API key is missing the constructor returns `None`; the
//! worker simply doesn't register this fetcher and the aggregator
//! flows through with the other two categories.

use async_trait::async_trait;
use serde::Deserialize;

use crate::types::{
    CategoryKind, CategoryReading, FetcherError, OnchainCategoryFetcher, OnchainDirection,
};

const BASE: &str = "https://api.glassnode.com";

#[derive(Debug, Clone, Copy)]
pub struct GlassnodeTuning {
    pub sopr_capitulation: f64, // <1 - this = bullish
    pub sopr_euphoria: f64,     // >1 + this = bearish
    pub mvrv_undervalued: f64,
    pub mvrv_overheated: f64,
}

impl Default for GlassnodeTuning {
    fn default() -> Self {
        Self {
            sopr_capitulation: 0.02,
            sopr_euphoria: 0.05,
            mvrv_undervalued: 1.0,
            mvrv_overheated: 3.5,
        }
    }
}

pub struct GlassnodeFetcher {
    client: reqwest::Client,
    api_key: String,
    tuning: GlassnodeTuning,
}

impl GlassnodeFetcher {
    /// Returns `None` when no API key is configured. Lets the worker
    /// soft-skip this fetcher without branching all over the place.
    pub fn new(
        client: reqwest::Client,
        api_key: Option<String>,
        tuning: GlassnodeTuning,
    ) -> Option<Self> {
        let key = api_key?.trim().to_string();
        if key.is_empty() {
            return None;
        }
        Some(Self { client, api_key: key, tuning })
    }
}

fn map_asset(symbol: &str) -> Result<&'static str, FetcherError> {
    let s = symbol.to_ascii_uppercase();
    let upper = s.as_str();
    if upper.starts_with("BTC") {
        Ok("BTC")
    } else if upper.starts_with("ETH") {
        Ok("ETH")
    } else {
        Err(FetcherError::UnsupportedSymbol(symbol.to_string()))
    }
}

#[derive(Deserialize)]
struct GnPoint {
    v: f64,
}

async fn last_value(
    client: &reqwest::Client,
    endpoint: &str,
    asset: &str,
    api_key: &str,
) -> Result<Option<f64>, FetcherError> {
    let pts: Vec<GnPoint> = client
        .get(format!("{BASE}{endpoint}"))
        .query(&[("a", asset), ("api_key", api_key)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(pts.last().map(|p| p.v))
}

#[async_trait]
impl OnchainCategoryFetcher for GlassnodeFetcher {
    fn name(&self) -> &'static str {
        "glassnode"
    }

    fn category(&self) -> CategoryKind {
        CategoryKind::Chain
    }

    fn cadence_s(&self) -> u64 {
        86_400 // v1 metrics default resolution = 24h
    }

    async fn fetch(&self, symbol: &str) -> Result<CategoryReading, FetcherError> {
        let asset = map_asset(symbol)?;

        let sopr = last_value(
            &self.client,
            "/v1/metrics/indicators/sopr",
            asset,
            &self.api_key,
        )
        .await
        .ok()
        .flatten();

        let netflow = last_value(
            &self.client,
            "/v1/metrics/transactions/transfers_volume_to_exchanges_sum",
            asset,
            &self.api_key,
        )
        .await
        .ok()
        .flatten();

        let mvrv = last_value(&self.client, "/v1/metrics/market/mvrv", asset, &self.api_key)
            .await
            .ok()
            .flatten();

        Ok(blend(sopr, netflow, mvrv, self.tuning))
    }
}

fn blend(
    sopr: Option<f64>,
    exchange_inflow: Option<f64>,
    mvrv: Option<f64>,
    t: GlassnodeTuning,
) -> CategoryReading {
    let mut score = 0.0_f64;
    let mut details = Vec::new();
    let mut have = 0u32;

    if let Some(s) = sopr {
        have += 1;
        if s <= 1.0 - t.sopr_capitulation {
            score += 0.4;
            details.push(format!("SOPR {s:.3} capitulation → bull"));
        } else if s >= 1.0 + t.sopr_euphoria {
            score -= 0.4;
            details.push(format!("SOPR {s:.3} euphoria → bear"));
        }
    }

    if let Some(nf) = exchange_inflow {
        have += 1;
        if nf < 0.0 {
            score += 0.3;
            details.push("exchange outflow (bullish)".into());
        } else if nf > 0.0 {
            score -= 0.3;
            details.push("exchange inflow (bearish)".into());
        }
    }

    if let Some(m) = mvrv {
        have += 1;
        if m <= t.mvrv_undervalued {
            score += 0.3;
            details.push(format!("MVRV {m:.2} undervalued → bull"));
        } else if m >= t.mvrv_overheated {
            score -= 0.3;
            details.push(format!("MVRV {m:.2} overheated → bear"));
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
        cadence_s: 86_400,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitulation_signals_bull() {
        let r = blend(Some(0.95), Some(-1.0), Some(0.9), GlassnodeTuning::default());
        assert!(r.score > 0.5);
        assert_eq!(r.direction, Some(OnchainDirection::Long));
    }

    #[test]
    fn euphoria_signals_bear() {
        let r = blend(Some(1.10), Some(1.0), Some(4.0), GlassnodeTuning::default());
        assert!(r.score < -0.5);
    }

    #[test]
    fn missing_key_returns_none() {
        assert!(GlassnodeFetcher::new(reqwest::Client::new(), None, GlassnodeTuning::default()).is_none());
        assert!(GlassnodeFetcher::new(reqwest::Client::new(), Some("".into()), GlassnodeTuning::default()).is_none());
    }

    #[test]
    fn unsupported_symbol() {
        assert!(matches!(map_asset("DOGEUSDT"), Err(FetcherError::UnsupportedSymbol(_))));
        assert_eq!(map_asset("BTCUSDT").unwrap(), "BTC");
        assert_eq!(map_asset("ETHUSDT").unwrap(), "ETH");
    }
}
