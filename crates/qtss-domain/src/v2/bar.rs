//! v2 OHLCV bar — venue/asset-class agnostic.
//!
//! Distinct from `crate::bar::TimestampBar` which is the legacy crypto
//! pipeline bar. v2 carries an `Instrument` reference plus the timeframe
//! the bar belongs to so the same struct flows through detectors for any
//! market without per-venue special-casing.

use crate::v2::instrument::Instrument;
use crate::v2::timeframe::Timeframe;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub instrument: Instrument,
    pub timeframe: Timeframe,
    /// Inclusive open time of the bar.
    pub open_time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    /// True once the bar has closed; live-forming bars carry `false`.
    pub closed: bool,
}

impl Bar {
    pub fn range(&self) -> Decimal {
        self.high - self.low
    }

    pub fn body(&self) -> Decimal {
        (self.close - self.open).abs()
    }

    pub fn is_bullish(&self) -> bool {
        self.close > self.open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::instrument::{AssetClass, SessionCalendar, Venue};
    use rust_decimal_macros::dec;

    fn fixture() -> Bar {
        Bar {
            instrument: Instrument {
                venue: Venue::Binance,
                asset_class: AssetClass::CryptoSpot,
                symbol: "BTCUSDT".into(),
                quote_ccy: "USDT".into(),
                tick_size: dec!(0.01),
                lot_size: dec!(0.00001),
                session: SessionCalendar::binance_24x7(),
            },
            timeframe: Timeframe::H4,
            open_time: Utc::now(),
            open: dec!(100),
            high: dec!(110),
            low: dec!(95),
            close: dec!(108),
            volume: dec!(1234.5),
            closed: true,
        }
    }

    #[test]
    fn range_and_body_are_decimal_safe() {
        let b = fixture();
        assert_eq!(b.range(), dec!(15));
        assert_eq!(b.body(), dec!(8));
        assert!(b.is_bullish());
    }

    #[test]
    fn json_round_trip() {
        let b = fixture();
        let j = serde_json::to_string(&b).unwrap();
        let back: Bar = serde_json::from_str(&j).unwrap();
        assert_eq!(b, back);
    }
}
