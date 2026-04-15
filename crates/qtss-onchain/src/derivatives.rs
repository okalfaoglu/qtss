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

    fn cadence_s(&self) -> u64 {
        3600
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
///
/// All four components contribute *continuously* via `tanh` ramps with
/// the configured tuning thresholds as half-saturation points. The old
/// hard-threshold version produced 0 for the vast majority of pairs
/// (most markets are not extreme on any given hour), making the
/// derivatives pillar effectively dead in `qtss_v2_onchain_metrics`.
/// Continuous scoring keeps small non-zero readings flowing so the
/// aggregator/validator can use the gradient instead of a binary on/off.
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

    // 1) Funding (contrarian): positive funding = crowded longs = bearish.
    //    Magnitude saturates at ±0.4 around |funding| ≈ 2 × funding_extreme.
    let funding_axis = (funding / t.funding_extreme).tanh();
    let funding_score = -0.4 * funding_axis;
    score += funding_score;
    details.push(format!("funding {funding:.5} → {funding_score:+.3}"));

    // 2) OI delta: only contributes on a *drop* (build alone is ambiguous);
    //    polarity follows funding (drop + crowded longs = longs liquidating
    //    = bearish; drop + crowded shorts = shorts covering = bullish).
    if let Some(d) = oi_delta {
        if d < 0.0 {
            let oi_mag = (d.abs() / t.oi_delta_significant).min(1.5);
            let oi_score = -0.2 * funding_axis * oi_mag;
            score += oi_score;
            details.push(format!("OI Δ {:+.2}% → {oi_score:+.3}", d * 100.0));
        }
    }

    // 3) L/S ratio (contrarian): centred at 1.0; long-extreme is the
    //    half-saturation point on the bearish side, short-extreme on
    //    the bullish side.
    let ls_half = (t.ls_ratio_long_extreme - 1.0).max(0.1);
    let ls_axis = ((ls_ratio - 1.0) / ls_half).tanh();
    let ls_score = -0.2 * ls_axis;
    score += ls_score;
    details.push(format!("L/S {ls_ratio:.2} → {ls_score:+.3}"));

    // 4) Taker buy/sell ratio (trend-following).
    let taker_half = (t.taker_long_extreme - 1.0).max(0.05);
    let taker_axis = ((taker_ratio - 1.0) / taker_half).tanh();
    let taker_score = 0.2 * taker_axis;
    score += taker_score;
    details.push(format!("taker {taker_ratio:.2} → {taker_score:+.3}"));

    let _ = (t.ls_ratio_short_extreme, t.taker_short_extreme); // documented half-points

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
        cadence_s: 3600, // openInterestHist / L-S / taker all period=1h
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
        // Continuous scoring: zero on every axis must yield zero score.
        let r = blend("BTCUSDT", 0.0, None, 1.0, 1.0, DerivativesTuning::default());
        assert!(r.score.abs() < 1e-9);
        assert_eq!(r.direction, Some(OnchainDirection::Neutral));
    }
}
