//! Confidence-threshold rule strategy.
//!
//! The simplest possible `StrategyProvider`: if the validated
//! detection is `Confirmed` and its blended confidence clears the
//! configured floor, emit one [`TradeIntent`] using the detection's
//! invalidation price as the stop and its own targets as the
//! take-profits. All numerics are config-driven (CLAUDE.md #2).

use crate::v2::error::{StrategyError, StrategyResult};
use crate::v2::provider::{StrategyContext, StrategyProvider};
use chrono::Utc;
use qtss_domain::v2::detection::{PatternState, ValidatedDetection};
use qtss_domain::v2::intent::{SizingHint, TimeInForce, TradeIntent};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ConfidenceThresholdStrategyConfig {
    /// Minimum blended confidence in 0..1 to fire.
    pub min_confidence: f32,
    /// Per-trade risk fraction passed through as `SizingHint::RiskPct`.
    pub risk_pct: Decimal,
    pub time_in_force: TimeInForce,
    pub time_stop_secs: Option<i64>,
    /// Whether to also act on `Forming` patterns. Most setups want
    /// `Confirmed` only — left configurable so research workflows can
    /// dry-run aggressive variants.
    pub act_on_forming: bool,
}

pub struct ConfidenceThresholdStrategy {
    id: String,
    config: ConfidenceThresholdStrategyConfig,
}

impl ConfidenceThresholdStrategy {
    pub fn new(id: impl Into<String>, config: ConfidenceThresholdStrategyConfig) -> StrategyResult<Self> {
        if !(0.0..=1.0).contains(&config.min_confidence) {
            return Err(StrategyError::InvalidConfig(
                "min_confidence must be in 0..1".into(),
            ));
        }
        if config.risk_pct <= Decimal::ZERO {
            return Err(StrategyError::InvalidConfig(
                "risk_pct must be positive".into(),
            ));
        }
        Ok(Self { id: id.into(), config })
    }

    fn passes_state(&self, state: PatternState) -> bool {
        match state {
            PatternState::Confirmed => true,
            PatternState::Forming => self.config.act_on_forming,
            PatternState::Invalidated | PatternState::Completed => false,
        }
    }

    fn side_for(&self, signal: &ValidatedDetection) -> qtss_domain::v2::intent::Side {
        // Direction inferred from invalidation vs nearest target: if
        // invalidation is below the first target → long; otherwise short.
        let invalidation = signal.detection.invalidation_price;
        let first_target = signal
            .detection
            .raw_meta
            .get("entry_price")
            .and_then(|v| v.as_f64())
            .and_then(Decimal::from_f64)
            .unwrap_or(invalidation);
        if first_target >= invalidation {
            qtss_domain::v2::intent::Side::Long
        } else {
            qtss_domain::v2::intent::Side::Short
        }
    }
}

impl StrategyProvider for ConfidenceThresholdStrategy {
    fn id(&self) -> &str {
        &self.id
    }

    fn evaluate(
        &self,
        signal: &ValidatedDetection,
        ctx: &StrategyContext,
    ) -> StrategyResult<Vec<TradeIntent>> {
        if !self.passes_state(signal.detection.state) {
            return Ok(vec![]);
        }
        if signal.confidence < self.config.min_confidence {
            return Ok(vec![]);
        }

        let intent = TradeIntent {
            id: Uuid::new_v4(),
            created_at: Utc::now(),
            strategy_id: self.id.clone(),
            instrument: signal.detection.instrument.clone(),
            timeframe: signal.detection.timeframe,
            side: self.side_for(signal),
            sizing: SizingHint::RiskPct { pct: self.config.risk_pct },
            entry_price: None, // market entry — risk layer / execution decides
            stop_loss: signal.detection.invalidation_price,
            take_profits: vec![], // target-engine fills these elsewhere
            time_in_force: self.config.time_in_force,
            time_stop_secs: self.config.time_stop_secs,
            source_signals: vec![signal.detection.id],
            conviction: signal.confidence,
            mode: ctx.run_mode,
        };
        Ok(vec![intent])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qtss_domain::execution::ExecutionMode;
    use qtss_domain::v2::detection::{
        ChannelScore, Detection, PatternKind, PatternState, ValidatedDetection,
    };
    use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};
    use qtss_domain::v2::instrument::{
        AssetClass, Instrument, SessionCalendar, Venue,
    };
    use qtss_domain::v2::timeframe::Timeframe;
    use rust_decimal_macros::dec;

    fn instrument() -> Instrument {
        Instrument {
            venue: Venue::Binance,
            asset_class: AssetClass::CryptoSpot,
            symbol: "BTCUSDT".into(),
            quote_ccy: "USDT".into(),
            tick_size: dec!(0.01),
            lot_size: dec!(0.00001),
            session: SessionCalendar::binance_24x7(),
        }
    }

    fn signal(state: PatternState, confidence: f32) -> ValidatedDetection {
        ValidatedDetection {
            detection: Detection {
                id: Uuid::new_v4(),
                instrument: instrument(),
                timeframe: Timeframe::H1,
                kind: PatternKind::Custom("test".into()),
                state,
                anchors: vec![],
                structural_score: 0.8,
                invalidation_price: dec!(49000),
                regime_at_detection: RegimeSnapshot {
                    at: Utc::now(),
                    kind: RegimeKind::TrendingUp,
                    trend_strength: TrendStrength::Strong,
                    adx: dec!(30),
                    bb_width: dec!(0.04),
                    atr_pct: dec!(0.02),
                    choppiness: dec!(40),
                    confidence: 0.8,
                },
                detected_at: Utc::now(),
                raw_meta: serde_json::json!({"entry_price": 50000.0}),
                projected_anchors: Vec::new(),
                sub_wave_anchors: Vec::new(),
                render_geometry: None,
                render_style: None,
                render_labels: None,
            },
            channel_scores: vec![ChannelScore {
                channel: "test".into(),
                score: 0.8,
                weight: 1.0,
            }],
            confidence,
            validated_at: Utc::now(),
        }
    }

    fn cfg() -> ConfidenceThresholdStrategyConfig {
        ConfidenceThresholdStrategyConfig {
            min_confidence: 0.7,
            risk_pct: dec!(0.005),
            time_in_force: TimeInForce::Gtc,
            time_stop_secs: Some(3600),
            act_on_forming: false,
        }
    }

    fn ctx() -> StrategyContext {
        StrategyContext { run_mode: ExecutionMode::Dry }
    }

    #[test]
    fn confirmed_above_threshold_emits_intent() {
        let s = ConfidenceThresholdStrategy::new("test", cfg()).unwrap();
        let intents = s
            .evaluate(&signal(PatternState::Confirmed, 0.9), &ctx())
            .unwrap();
        assert_eq!(intents.len(), 1);
        let i = &intents[0];
        assert_eq!(i.strategy_id, "test");
        assert_eq!(i.stop_loss, dec!(49000));
        assert_eq!(i.side, qtss_domain::v2::intent::Side::Long);
    }

    #[test]
    fn below_threshold_passes() {
        let s = ConfidenceThresholdStrategy::new("test", cfg()).unwrap();
        let intents = s
            .evaluate(&signal(PatternState::Confirmed, 0.5), &ctx())
            .unwrap();
        assert!(intents.is_empty());
    }

    #[test]
    fn forming_default_blocks() {
        let s = ConfidenceThresholdStrategy::new("test", cfg()).unwrap();
        let intents = s
            .evaluate(&signal(PatternState::Forming, 0.95), &ctx())
            .unwrap();
        assert!(intents.is_empty());
    }

    #[test]
    fn invalidated_blocks_regardless_of_confidence() {
        let s = ConfidenceThresholdStrategy::new("test", cfg()).unwrap();
        let intents = s
            .evaluate(&signal(PatternState::Invalidated, 0.99), &ctx())
            .unwrap();
        assert!(intents.is_empty());
    }

    #[test]
    fn invalid_config_rejected() {
        let mut bad = cfg();
        bad.min_confidence = 1.5;
        assert!(ConfidenceThresholdStrategy::new("x", bad).is_err());
    }
}
