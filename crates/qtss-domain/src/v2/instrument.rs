//! Asset-class agnostic instrument model.
//!
//! Distinct from the legacy `InstrumentId` (crypto-centric, just an exchange
//! + symbol pair). v2 carries the full venue + asset-class + session +
//! tick/lot metadata so detector / risk / execution layers don't have to
//! special-case venues with hardcoded constants (CLAUDE.md rule #2).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Trading venue identifier. Open enum: `Custom(String)` lets new venues
/// land without touching this file (rule #1: no scattered if/else over
/// venue lists).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Venue {
    Binance,
    Bist,
    Nasdaq,
    Nyse,
    Bybit,
    Okx,
    Polygon,
    Alpaca,
    Ibkr,
    Custom(String),
}

impl Venue {
    /// Stable string key (used for config_scope, canonical ids, log fields).
    pub fn as_key(&self) -> &str {
        match self {
            Venue::Binance => "binance",
            Venue::Bist => "bist",
            Venue::Nasdaq => "nasdaq",
            Venue::Nyse => "nyse",
            Venue::Bybit => "bybit",
            Venue::Okx => "okx",
            Venue::Polygon => "polygon",
            Venue::Alpaca => "alpaca",
            Venue::Ibkr => "ibkr",
            Venue::Custom(s) => s.as_str(),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumString, Display,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AssetClass {
    CryptoSpot,
    CryptoFutures,
    CryptoMargin,
    CryptoOptions,
    EquityBist,
    EquityNasdaq,
    EquityNyse,
    Forex,
    Commodity,
}

impl AssetClass {
    /// Stable string used for `config_scope.scope_key` lookups.
    pub fn config_scope_key(self) -> &'static str {
        self.into()
    }
}

impl From<AssetClass> for &'static str {
    fn from(c: AssetClass) -> &'static str {
        match c {
            AssetClass::CryptoSpot => "crypto_spot",
            AssetClass::CryptoFutures => "crypto_futures",
            AssetClass::CryptoMargin => "crypto_margin",
            AssetClass::CryptoOptions => "crypto_options",
            AssetClass::EquityBist => "equity_bist",
            AssetClass::EquityNasdaq => "equity_nasdaq",
            AssetClass::EquityNyse => "equity_nyse",
            AssetClass::Forex => "forex",
            AssetClass::Commodity => "commodity",
        }
    }
}

/// Trading session calendar reference. We don't embed the actual schedule
/// here — that's a separate `qtss-calendar` concern. This struct just names
/// the calendar so the right one can be looked up by id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionCalendar {
    /// Stable id resolved against the calendars table (e.g. "binance_24x7",
    /// "bist_main", "nasdaq_regular", "nasdaq_extended").
    pub id: String,
}

impl SessionCalendar {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }

    pub fn binance_24x7() -> Self {
        Self::new("binance_24x7")
    }
    pub fn bist_main() -> Self {
        Self::new("bist_main")
    }
    pub fn nasdaq_regular() -> Self {
        Self::new("nasdaq_regular")
    }
}

/// Asset-class agnostic instrument. Used by detectors, risk, execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instrument {
    pub venue: Venue,
    pub asset_class: AssetClass,
    /// Native venue symbol (e.g. "BTCUSDT", "THYAO", "AAPL").
    pub symbol: String,
    pub quote_ccy: String,
    pub tick_size: Decimal,
    pub lot_size: Decimal,
    pub session: SessionCalendar,
}

impl Instrument {
    /// Stable canonical id for hashing / event correlation.
    pub fn canonical_id(&self) -> String {
        format!(
            "{}:{}:{}",
            self.venue.as_key(),
            self.asset_class.config_scope_key(),
            self.symbol
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn canonical_id_format_is_stable() {
        let inst = Instrument {
            venue: Venue::Binance,
            asset_class: AssetClass::CryptoSpot,
            symbol: "BTCUSDT".into(),
            quote_ccy: "USDT".into(),
            tick_size: dec!(0.01),
            lot_size: dec!(0.00001),
            session: SessionCalendar::binance_24x7(),
        };
        assert_eq!(inst.canonical_id(), "binance:crypto_spot:BTCUSDT");
    }

    #[test]
    fn custom_venue_round_trips() {
        let v = Venue::Custom("dydx".into());
        let j = serde_json::to_string(&v).unwrap();
        let back: Venue = serde_json::from_str(&j).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn asset_class_scope_key_matches_config_seed() {
        // These keys are used by config_scope (migration 0014).
        assert_eq!(AssetClass::CryptoSpot.config_scope_key(), "crypto_spot");
        assert_eq!(AssetClass::EquityBist.config_scope_key(), "equity_bist");
    }
}
