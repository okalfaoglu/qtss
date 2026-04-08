//! Timeframe enum used by detectors, indicators, and the bar feed.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumString, Display,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Timeframe {
    M1,
    M3,
    M5,
    M15,
    M30,
    H1,
    H2,
    H4,
    H6,
    H8,
    H12,
    D1,
    D3,
    W1,
    Mn1,
}

impl Timeframe {
    /// Length in seconds. Pure helper, no clock dependency.
    pub fn seconds(self) -> i64 {
        match self {
            Timeframe::M1 => 60,
            Timeframe::M3 => 180,
            Timeframe::M5 => 300,
            Timeframe::M15 => 900,
            Timeframe::M30 => 1_800,
            Timeframe::H1 => 3_600,
            Timeframe::H2 => 7_200,
            Timeframe::H4 => 14_400,
            Timeframe::H6 => 21_600,
            Timeframe::H8 => 28_800,
            Timeframe::H12 => 43_200,
            Timeframe::D1 => 86_400,
            Timeframe::D3 => 259_200,
            Timeframe::W1 => 604_800,
            // Calendar months are not constant; use 30 days as a stable
            // approximation for ranking / sizing only.
            Timeframe::Mn1 => 2_592_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lowercase() {
        let tf: Timeframe = "h4".parse().unwrap();
        assert_eq!(tf, Timeframe::H4);
    }

    #[test]
    fn seconds_monotone_within_intraday() {
        assert!(Timeframe::M1.seconds() < Timeframe::M5.seconds());
        assert!(Timeframe::M5.seconds() < Timeframe::H1.seconds());
        assert!(Timeframe::H1.seconds() < Timeframe::D1.seconds());
    }
}
