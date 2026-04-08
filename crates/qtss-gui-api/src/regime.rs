//! `/v2/regime/{venue}/{symbol}/{tf}` wire types -- Faz 5 Adim (d).
//!
//! The Regime HUD shows the current market regime (trending, ranging,
//! squeeze, volatile, ...) plus the indicator values that drove the
//! classification, plus a short history strip so the user can see
//! how stable the regime has been.
//!
//! These DTOs wrap (not re-export) `RegimeSnapshot` so the wire shape
//! stays under our control as the engine evolves -- if a future
//! qtss-regime adds fields, we splice them in here without breaking
//! the contract.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};

/// One regime snapshot in wire form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegimeView {
    pub at: DateTime<Utc>,
    pub kind: RegimeKind,
    pub trend_strength: TrendStrength,
    pub adx: Decimal,
    pub bb_width: Decimal,
    pub atr_pct: Decimal,
    pub choppiness: Decimal,
    pub confidence: f32,
}

impl From<RegimeSnapshot> for RegimeView {
    fn from(s: RegimeSnapshot) -> Self {
        Self {
            at: s.at,
            kind: s.kind,
            trend_strength: s.trend_strength,
            adx: s.adx,
            bb_width: s.bb_width,
            atr_pct: s.atr_pct,
            choppiness: s.choppiness,
            confidence: s.confidence,
        }
    }
}

/// Compact history-strip point. Stripped down on purpose so the
/// sparkline payload stays cheap; the user pivots to detail via the
/// `current` block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegimePoint {
    pub at: DateTime<Utc>,
    pub kind: RegimeKind,
    pub confidence: f32,
}

impl From<&RegimeSnapshot> for RegimePoint {
    fn from(s: &RegimeSnapshot) -> Self {
        Self { at: s.at, kind: s.kind, confidence: s.confidence }
    }
}

/// Whole `/v2/regime/...` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegimeHud {
    pub generated_at: DateTime<Utc>,
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    /// `None` while the engine is still in its warm-up window. The
    /// frontend should render a "warming up" placeholder.
    pub current: Option<RegimeView>,
    /// Newest-last sparkline of the last few classifications.
    pub history: Vec<RegimePoint>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn snap(kind: RegimeKind) -> RegimeSnapshot {
        RegimeSnapshot {
            at: Utc::now(),
            kind,
            trend_strength: TrendStrength::Strong,
            adx: dec!(30),
            bb_width: dec!(0.04),
            atr_pct: dec!(0.02),
            choppiness: dec!(40),
            confidence: 0.8,
        }
    }

    #[test]
    fn view_round_trip_through_from() {
        let s = snap(RegimeKind::TrendingUp);
        let v: RegimeView = s.clone().into();
        assert_eq!(v.kind, s.kind);
        assert_eq!(v.adx, s.adx);
    }

    #[test]
    fn point_is_lightweight() {
        let s = snap(RegimeKind::Ranging);
        let p: RegimePoint = (&s).into();
        assert_eq!(p.kind, RegimeKind::Ranging);
    }

    #[test]
    fn json_round_trip() {
        let hud = RegimeHud {
            generated_at: Utc::now(),
            venue: "binance".into(),
            symbol: "BTCUSDT".into(),
            timeframe: "1h".into(),
            current: Some(snap(RegimeKind::Squeeze).into()),
            history: vec![(&snap(RegimeKind::Ranging)).into()],
        };
        let j = serde_json::to_string(&hud).unwrap();
        let back: RegimeHud = serde_json::from_str(&j).unwrap();
        assert_eq!(back.history.len(), 1);
        assert!(back.current.is_some());
    }
}
