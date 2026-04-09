//! Binance public derivatives fetcher (free, no API key).
//!
//! We pull four signals and blend them into one `[-1, +1]` score:
//! 1. Funding rate — extreme positive = over-leveraged longs (bearish).
//! 2. Open interest delta — sharp drop = capitulation/squeeze.
//! 3. Global long/short ratio — crowd positioning, contrarian.
//! 4. Taker buy/sell ratio — aggressive flow direction.
//!
//! Endpoints (USDT-M futures):
//! - `/fapi/v1/premiumIndex?symbol=BTCUSDT` (lastFundingRate)
//! - `/futures/data/openInterestHist?symbol=...&period=1h&limit=2`
//! - `/futures/data/globalLongShortAccountRatio?...&period=1h&limit=1`
//! - `/futures/data/takerlongshortRatio?...&period=1h&limit=1`
//!
//! All thresholds are config-driven (CLAUDE.md #2): the worker passes
//! [`DerivativesTuning`] resolved from `system_config`, defaults are
//! kept here only for tests.

use async_trait::async_trait;
use serde::Deserialize;

use crate::types::{
    CategoryKind, CategoryReading, FetcherError, OnchainCategoryFetcher, OnchainDirection,
};

const BASE: &str = "https://fapi.binance.com";

#[derive(Debug, Clone, Copy)]
pub struct DerivativesTuning {
    /// |funding| above this is treated as extreme.
    pub funding_extreme: f64,
    /// OI percent change considered structurally significant.
    pub oi_delta_significant: f64,
    /// Long/short ratio above this = crowd long (bearish contrarian).
    pub ls_ratio_long_extreme: f64,
    /// Long/short ratio below this = crowd short (bullish contrarian).
    pub ls_ratio_short_extreme: f64,
    /// Taker buy/sell ratio above this = aggressive buying.
    pub taker_long_extreme: f64,
    /// Taker buy/sell ratio below this = aggressive selling.
    pub taker_short_extreme: f64,
}

impl Default for DerivativesTuning {
    fn default() -> Self {
        Self {
            funding_extreme: 0.0005,
            oi_delta_significant: 0.05,
            ls_ratio_long_extreme: 2.5,
            ls_ratio_short_extreme: 0.5,
            taker_long_extreme: 1.3,
            taker_short_extreme: 0.7,
        }
    }
}

pub struct BinanceDerivativesFetcher {
    client: reqwest::Client,
    tuning: DerivativesTuning,
}

impl BinanceDerivativesFetcher {
    pub fn new(client: reqwest::Client, tuning: DerivativesTuning) -> Self {
        Self { client, tuning }
    }

    pub fn with_defaults() -> Self {
        Self::new(reqwest::Client::new(), DerivativesTuning::default())
    }
}

#[derive(Deserialize)]
struct PremiumIndex {
    #[serde(rename = "lastFundingRate")]
    last_funding_rate: String,
}

#[derive(Deserialize)]
struct OiPoint {
    #[serde(rename = "sumOpenInterest")]
    sum_open_interest: String,
}

#[derive(Deserialize)]
struct LongShortPoint {
    #[serde(rename = "longShortRatio")]
    long_short_ratio: String,
}

#[derive(Deserialize)]
struct TakerPoint {
    #[serde(rename = "buySellRatio")]
    buy_sell_ratio: String,
}

async fn parse_first<T: for<'de> Deserialize<'de>>(
    resp: reqwest::Response,
) -> Result<T, FetcherError> {
    let mut v: Vec<T> = resp.json().await?;
    v.pop().ok_or_else(|| FetcherError::Decode("empty array".into()))
}

fn parse_f64(s: &str) -> Result<f64, FetcherError> {
    s.parse::<f64>().map_err(|e| FetcherError::Decode(e.to_string()))
}

#[async_trait]
impl OnchainCategoryFetcher for BinanceDerivativesFetcher {
    fn name(&self) -> &'static str {
        "binance_derivatives"
    }

    fn category(&self) -> CategoryKind {
        CategoryKind::Derivatives
    }

    async fn fetch(&self, symbol: &str) -> Result<CategoryReading, FetcherError> {
        // 1) Funding rate
        let funding: f64 = {
            let r: PremiumIndex = self
                .client
                .get(format!("{BASE}/fapi/v1/premiumIndex"))
                .query(&[("symbol", symbol)])
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            parse_f64(&r.last_funding_rate)?
        };

        // 2) Open interest delta (last vs prev hour)
        let oi_delta: Option<f64> = {
            let resp = self
                .client
                .get(format!("{BASE}/futures/data/openInterestHist"))
                .query(&[("symbol", symbol), ("period", "1h"), ("limit", "2")])
                .send()
                .await?
                .error_for_status()?;
            let pts: Vec<OiPoint> = resp.json().await?;
            if pts.len() == 2 {
                let prev = parse_f64(&pts[0].sum_open_interest)?;
                let cur = parse_f64(&pts[1].sum_open_interest)?;
                if prev > 0.0 {
                    Some((cur - prev) / prev)
                } else {
                    None
                }
            } else {
                None
            }
        };

        // 3) Global long/short ratio
        let ls_ratio: f64 = {
            let resp = self
                .client
                .get(format!("{BASE}/futures/data/globalLongShortAccountRatio"))
                .query(&[("symbol", symbol), ("period", "1h"), ("limit", "1")])
                .send()
                .await?
                .error_for_status()?;
            let pt: LongShortPoint = parse_first(resp).await?;
            parse_f64(&pt.long_short_ratio)?
        };

        // 4) Taker buy/sell ratio
        let taker_ratio: f64 = {
            let resp = self
                .client
                .get(format!("{BASE}/futures/data/takerlongshortRatio"))
                .query(&[("symbol", symbol), ("period", "1h"), ("limit", "1")])
                .send()
                .await?
                .error_for_status()?;
            let pt: TakerPoint = parse_first(resp).await?;
            parse_f64(&pt.buy_sell_ratio)?
        };

        Ok(blend(
            symbol,
            funding,
            oi_delta,
            ls_ratio,
            taker_ratio,
            self.tuning,
        ))
    }
}

/// Pure scoring — broken out so unit tests can hit it without HTTP.
fn blend(
    symbol: &str,
    funding: f64,
    oi_delta: Option<f64>,
    ls_ratio: f64,
    taker_ratio: f64,
    t: DerivativesTuning,
) -> CategoryReading {
    let mut score = 0.0_f64;
    let mut details = Vec::new();

    // Funding: extreme positive → bearish (contrarian)
    if funding.abs() >= t.funding_extreme {
        let sign = if funding > 0.0 { -1.0 } else { 1.0 };
        score += 0.4 * sign;
        details.push(format!("funding {funding:.5} extreme → {}", if sign > 0.0 { "bull" } else { "bear" }));
    }

    // OI delta: sharp drop while leveraged → squeeze/capitulation
    if let Some(d) = oi_delta {
        if d.abs() >= t.oi_delta_significant {
            // Drop in OI on negative funding = shorts covering = bullish.
            // Drop on positive funding = longs liquidating = bearish.
            let sign = if d < 0.0 {
                if funding < 0.0 { 1.0 } else { -1.0 }
            } else {
                0.0 // OI build alone is ambiguous
            };
            if sign != 0.0 {
                score += 0.2 * sign;
                details.push(format!("OI Δ {:+.2}% ({})", d * 100.0, if sign > 0.0 { "bull" } else { "bear" }));
            }
        }
    }

    // L/S ratio: contrarian
    if ls_ratio >= t.ls_ratio_long_extreme {
        score -= 0.2;
        details.push(format!("L/S {ls_ratio:.2} crowded long → bear"));
    } else if ls_ratio <= t.ls_ratio_short_extreme {
        score += 0.2;
        details.push(format!("L/S {ls_ratio:.2} crowded short → bull"));
    }

    // Taker flow: trend-following
    if taker_ratio >= t.taker_long_extreme {
        score += 0.2;
        details.push(format!("taker {taker_ratio:.2} buy-dominant"));
    } else if taker_ratio <= t.taker_short_extreme {
        score -= 0.2;
        details.push(format!("taker {taker_ratio:.2} sell-dominant"));
    }

    let score = score.clamp(-1.0, 1.0);
    let direction = if score > 0.05 {
        OnchainDirection::Long
    } else if score < -0.05 {
        OnchainDirection::Short
    } else {
        OnchainDirection::Neutral
    };

    let _ = symbol;
    CategoryReading {
        category: CategoryKind::Derivatives,
        score,
        confidence: 0.9, // Binance public is reliable; only stale on outage.
        direction: Some(direction),
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extreme_positive_funding_is_bearish() {
        let r = blend("BTCUSDT", 0.001, None, 1.0, 1.0, DerivativesTuning::default());
        assert!(r.score < 0.0);
        assert_eq!(r.direction, Some(OnchainDirection::Short));
    }

    #[test]
    fn crowded_short_plus_short_covering_is_bullish() {
        let r = blend(
            "BTCUSDT",
            -0.001,
            Some(-0.10),
            0.4,
            0.6,
            DerivativesTuning::default(),
        );
        assert!(r.score > 0.0);
        assert_eq!(r.direction, Some(OnchainDirection::Long));
    }

    #[test]
    fn neutral_inputs_neutral_output() {
        let r = blend("BTCUSDT", 0.0001, None, 1.0, 1.0, DerivativesTuning::default());
        assert_eq!(r.direction, Some(OnchainDirection::Neutral));
    }
}
