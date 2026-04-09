//! Macro liquidity fetcher — DeFiLlama stablecoins + alternative.me F&G.
//!
//! Both endpoints are free, market-wide (not symbol-specific) and only
//! refresh every few hours. We treat them as a single "macro" reading
//! that gates aggressive bottom calls during liquidity contractions
//! and aggressive top calls during euphoria.
//!
//! - DeFiLlama: `https://stablecoins.llama.fi/stablecoincharts/all`
//!   returns daily total stablecoin market cap. We compute the 7-day
//!   delta — expanding stablecoin supply = fresh capital ready to bid.
//! - alternative.me: `https://api.alternative.me/fng/?limit=1` returns
//!   the Fear & Greed index `[0..100]`. <25 = extreme fear (bullish
//!   contrarian), >75 = extreme greed (bearish contrarian).

use async_trait::async_trait;
use serde::Deserialize;

use crate::types::{
    CategoryKind, CategoryReading, FetcherError, OnchainCategoryFetcher, OnchainDirection,
};

#[derive(Debug, Clone, Copy)]
pub struct StablecoinTuning {
    pub stable_growth_bullish: f64, // 7d Δ above this = bullish
    pub stable_shrink_bearish: f64, // 7d Δ below this = bearish
    pub fng_extreme_fear: u32,
    pub fng_extreme_greed: u32,
}

impl Default for StablecoinTuning {
    fn default() -> Self {
        Self {
            stable_growth_bullish: 0.01,
            stable_shrink_bearish: -0.01,
            fng_extreme_fear: 25,
            fng_extreme_greed: 75,
        }
    }
}

pub struct StablecoinMacroFetcher {
    client: reqwest::Client,
    tuning: StablecoinTuning,
}

impl StablecoinMacroFetcher {
    pub fn new(client: reqwest::Client, tuning: StablecoinTuning) -> Self {
        Self { client, tuning }
    }

    pub fn with_defaults() -> Self {
        Self::new(reqwest::Client::new(), StablecoinTuning::default())
    }
}

#[derive(Deserialize)]
struct LlamaPoint {
    #[serde(rename = "totalCirculatingUSD")]
    total: LlamaPeg,
}

#[derive(Deserialize)]
struct LlamaPeg {
    #[serde(rename = "peggedUSD")]
    pegged_usd: f64,
}

#[derive(Deserialize)]
struct FngResp {
    data: Vec<FngPoint>,
}

#[derive(Deserialize)]
struct FngPoint {
    value: String,
}

async fn fetch_stable_delta(client: &reqwest::Client) -> Result<Option<f64>, FetcherError> {
    let pts: Vec<LlamaPoint> = client
        .get("https://stablecoins.llama.fi/stablecoincharts/all")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if pts.len() < 8 {
        return Ok(None);
    }
    let last = pts[pts.len() - 1].total.pegged_usd;
    let prev = pts[pts.len() - 8].total.pegged_usd;
    if prev <= 0.0 {
        return Ok(None);
    }
    Ok(Some((last - prev) / prev))
}

async fn fetch_fng(client: &reqwest::Client) -> Result<Option<u32>, FetcherError> {
    let r: FngResp = client
        .get("https://api.alternative.me/fng/?limit=1")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let Some(p) = r.data.first() else { return Ok(None) };
    Ok(p.value.parse::<u32>().ok())
}

#[async_trait]
impl OnchainCategoryFetcher for StablecoinMacroFetcher {
    fn name(&self) -> &'static str {
        "stablecoin_macro"
    }

    fn category(&self) -> CategoryKind {
        CategoryKind::Stablecoin
    }

    async fn fetch(&self, _symbol: &str) -> Result<CategoryReading, FetcherError> {
        let stable_delta = fetch_stable_delta(&self.client).await.ok().flatten();
        let fng = fetch_fng(&self.client).await.ok().flatten();
        Ok(blend(stable_delta, fng, self.tuning))
    }
}

fn blend(
    stable_delta: Option<f64>,
    fng: Option<u32>,
    t: StablecoinTuning,
) -> CategoryReading {
    let mut score = 0.0_f64;
    let mut details = Vec::new();
    let mut have = 0u32;

    if let Some(d) = stable_delta {
        have += 1;
        if d >= t.stable_growth_bullish {
            score += 0.5;
            details.push(format!("stablecoin 7d Δ {:+.2}% (liquidity expanding)", d * 100.0));
        } else if d <= t.stable_shrink_bearish {
            score -= 0.5;
            details.push(format!("stablecoin 7d Δ {:+.2}% (liquidity draining)", d * 100.0));
        }
    }

    if let Some(f) = fng {
        have += 1;
        if f <= t.fng_extreme_fear {
            score += 0.5;
            details.push(format!("F&G {f} extreme fear → bull contrarian"));
        } else if f >= t.fng_extreme_greed {
            score -= 0.5;
            details.push(format!("F&G {f} extreme greed → bear contrarian"));
        }
    }

    let score = score.clamp(-1.0, 1.0);
    let confidence = match have {
        0 => 0.0,
        1 => 0.4,
        _ => 0.7,
    };
    let direction = if score > 0.05 {
        OnchainDirection::Long
    } else if score < -0.05 {
        OnchainDirection::Short
    } else {
        OnchainDirection::Neutral
    };

    CategoryReading {
        category: CategoryKind::Stablecoin,
        score,
        confidence,
        direction: Some(direction),
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fear_plus_growth_is_bullish() {
        let r = blend(Some(0.02), Some(15), StablecoinTuning::default());
        assert!(r.score > 0.5);
        assert_eq!(r.direction, Some(OnchainDirection::Long));
    }

    #[test]
    fn greed_plus_drain_is_bearish() {
        let r = blend(Some(-0.02), Some(85), StablecoinTuning::default());
        assert!(r.score < -0.5);
    }

    #[test]
    fn no_data_zero_confidence() {
        let r = blend(None, None, StablecoinTuning::default());
        assert_eq!(r.confidence, 0.0);
    }
}
