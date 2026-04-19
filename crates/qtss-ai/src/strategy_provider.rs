//! AI as a v2 [`StrategyProvider`].
//!
//! Plan §10 Faz 4B requires the AI engine to be refactored from a
//! standalone "decision generator" into something that plugs into the
//! same DSL/strategy plane every other strategy uses. Concretely:
//!
//! - The worker hands a `ValidatedDetection` + `StrategyContext` to a
//!   strategy. For an AI strategy, that goes through this provider.
//! - This provider asks an injected [`LlmAdvisor`] for a verdict
//!   ("act"/"hold" + conviction in `0..1`).
//! - On *act*, the provider returns a [`TradeIntent`] exactly like a
//!   rule strategy would; on *hold*, it returns the empty vec.
//! - On any error from the advisor, the provider degrades to a
//!   *hold* (an AI strategy must never be a hard dependency on the
//!   trading loop), but logs through `tracing` so the operator sees
//!   it. The kill-switch and risk gates downstream stay authoritative.
//!
//! ## Design (CLAUDE.md)
//!
//! - **Trait boundary (#3):** the only thing this module knows about
//!   the rest of qtss-ai is the [`LlmAdvisor`] trait — a one-method
//!   contract. The big `client.rs` / `providers/` machinery does *not*
//!   live in the trade loop; whoever wires the worker decides which
//!   advisor implements that trait (mock for tests, real provider for
//!   live, recorded fixture for backtest).
//! - **No hardcoded numbers (#2):** every threshold, default sizing
//!   fraction, and `auto_approve` flag is on
//!   [`AiStrategyProviderConfig`], populated from `qtss_config` by the
//!   worker at boot.
//! - **No scattered match arms (#1):** verdict → action is dispatched
//!   in one place via [`AiStrategyVerdict::is_actionable`].

use async_trait::async_trait;
use chrono::Utc;
use qtss_domain::v2::detection::ValidatedDetection;
use qtss_domain::v2::intent::{SizingHint, TimeInForce, TradeIntent};
use qtss_strategy::{
    StrategyContext, StrategyError, StrategyProvider, StrategyResult,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What an LLM-style advisor returned for one signal. Kept tiny on
/// purpose: anything richer (raw prompt, completion text, token
/// usage, ...) belongs in the AI engine's audit storage, not in the
/// trade loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiStrategyVerdict {
    /// True when the advisor believes the signal warrants entering.
    pub act: bool,
    /// Conviction in `0..1`. Combined with the validator's confidence
    /// to gate the final decision.
    pub conviction: f32,
    /// Free-form rationale; surfaced in the audit log only.
    pub rationale: String,
}

impl AiStrategyVerdict {
    pub fn is_actionable(&self, min_conviction: f32) -> bool {
        self.act && self.conviction >= min_conviction
    }
}

/// One-method contract the AI engine implements. Async because the
/// real provider hits a network LLM; tests inject a synchronous mock.
#[async_trait]
pub trait LlmAdvisor: Send + Sync {
    async fn evaluate(
        &self,
        signal: &ValidatedDetection,
        ctx: &StrategyContext,
    ) -> Result<AiStrategyVerdict, String>;
}

#[derive(Debug, Clone)]
pub struct AiStrategyProviderConfig {
    /// Validator confidence floor — same gate the rule strategy uses.
    pub min_validator_confidence: f32,
    /// Advisor conviction floor in `0..1`.
    pub min_ai_conviction: f32,
    /// Per-trade risk fraction passed through as `SizingHint::RiskPct`.
    pub risk_pct: Decimal,
    pub time_in_force: TimeInForce,
    pub time_stop_secs: Option<i64>,
}

pub struct AiStrategyProvider {
    id: String,
    config: AiStrategyProviderConfig,
    advisor: std::sync::Arc<dyn LlmAdvisor>,
}

impl AiStrategyProvider {
    pub fn new(
        id: impl Into<String>,
        config: AiStrategyProviderConfig,
        advisor: std::sync::Arc<dyn LlmAdvisor>,
    ) -> StrategyResult<Self> {
        if !(0.0..=1.0).contains(&config.min_validator_confidence)
            || !(0.0..=1.0).contains(&config.min_ai_conviction)
        {
            return Err(StrategyError::InvalidConfig(
                "confidence floors must be in 0..1".into(),
            ));
        }
        if config.risk_pct <= Decimal::ZERO {
            return Err(StrategyError::InvalidConfig(
                "risk_pct must be positive".into(),
            ));
        }
        Ok(Self { id: id.into(), config, advisor })
    }

    fn build_intent(
        &self,
        signal: &ValidatedDetection,
        verdict: &AiStrategyVerdict,
        ctx: &StrategyContext,
    ) -> TradeIntent {
        // Direction inference identical to the rule strategy: if the
        // raw_meta carries an entry hint above the invalidation, go
        // long; otherwise short. Strategies should not invent prices,
        // execution decides the actual entry.
        let invalidation = signal.detection.invalidation_price;
        let entry_hint = signal
            .detection
            .raw_meta
            .get("entry_price")
            .and_then(|v| v.as_f64())
            .and_then(rust_decimal::prelude::FromPrimitive::from_f64)
            .unwrap_or(invalidation);
        let side = if entry_hint >= invalidation {
            qtss_domain::v2::intent::Side::Long
        } else {
            qtss_domain::v2::intent::Side::Short
        };
        TradeIntent {
            id: Uuid::new_v4(),
            created_at: Utc::now(),
            strategy_id: self.id.clone(),
            instrument: signal.detection.instrument.clone(),
            timeframe: signal.detection.timeframe,
            side,
            sizing: SizingHint::RiskPct { pct: self.config.risk_pct },
            entry_price: None,
            stop_loss: invalidation,
            take_profits: vec![],
            time_in_force: self.config.time_in_force,
            time_stop_secs: self.config.time_stop_secs,
            source_signals: vec![signal.detection.id],
            // Conviction surfaced to risk = blended floor of the two
            // confidences so a single weak side cannot oversell the
            // intent. Risk still has the final word.
            conviction: signal.confidence.min(verdict.conviction),
            mode: ctx.run_mode,
        }
    }
}

/// `StrategyProvider` is sync; we tunnel into the async advisor by
/// blocking on a single-shot inside `tokio::task::block_in_place`
/// when a runtime is available, or `futures::executor::block_on`
/// otherwise. The provider is expected to run inside the worker's
/// tokio runtime (the only place strategies actually fire), so the
/// fast path is `block_in_place`.
fn block_on_advisor<F: std::future::Future<Output = T>, T>(fut: F) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => futures_runtime::block_on(fut),
    }
}

mod futures_runtime {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread;

    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    /// Minimal block_on for the no-runtime fallback (tests).
    pub fn block_on<F: Future>(fut: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut cx = Context::from_waker(&waker);
        let fut = Mutex::new(Box::pin(fut));
        loop {
            let mut guard = fut.lock().unwrap();
            let pinned: Pin<&mut _> = guard.as_mut();
            match pinned.poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {
                    drop(guard);
                    thread::park();
                }
            }
        }
    }
}

impl StrategyProvider for AiStrategyProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn evaluate(
        &self,
        signal: &ValidatedDetection,
        ctx: &StrategyContext,
    ) -> StrategyResult<Vec<TradeIntent>> {
        // Cheap pre-filter — never burn an LLM call on a low-confidence
        // detection.
        if signal.confidence < self.config.min_validator_confidence {
            return Ok(vec![]);
        }

        let advisor = self.advisor.clone();
        let verdict = block_on_advisor(advisor.evaluate(signal, ctx));
        let verdict = match verdict {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(strategy = %self.id, error = %e, "ai advisor failed — holding");
                return Ok(vec![]);
            }
        };

        if !verdict.is_actionable(self.config.min_ai_conviction) {
            return Ok(vec![]);
        }
        Ok(vec![self.build_intent(signal, &verdict, ctx)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qtss_domain::execution::ExecutionMode;
    use qtss_domain::v2::detection::{
        ChannelScore, Detection, PatternKind, PatternState,
    };
    use qtss_domain::v2::instrument::{
        AssetClass, Instrument, SessionCalendar, Venue,
    };
    use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};
    use qtss_domain::v2::timeframe::Timeframe;
    use rust_decimal_macros::dec;
    use std::sync::Arc;

    struct StubAdvisor(AiStrategyVerdict);
    #[async_trait]
    impl LlmAdvisor for StubAdvisor {
        async fn evaluate(
            &self,
            _signal: &ValidatedDetection,
            _ctx: &StrategyContext,
        ) -> Result<AiStrategyVerdict, String> {
            Ok(self.0.clone())
        }
    }

    struct FailingAdvisor;
    #[async_trait]
    impl LlmAdvisor for FailingAdvisor {
        async fn evaluate(
            &self,
            _signal: &ValidatedDetection,
            _ctx: &StrategyContext,
        ) -> Result<AiStrategyVerdict, String> {
            Err("network down".into())
        }
    }

    fn signal(confidence: f32) -> ValidatedDetection {
        ValidatedDetection {
            detection: Detection {
                id: Uuid::new_v4(),
                instrument: Instrument {
                    venue: Venue::Binance,
                    asset_class: AssetClass::CryptoSpot,
                    symbol: "BTCUSDT".into(),
                    quote_ccy: "USDT".into(),
                    tick_size: dec!(0.01),
                    lot_size: dec!(0.00001),
                    session: SessionCalendar::binance_24x7(),
                },
                timeframe: Timeframe::H1,
                kind: PatternKind::Custom("ai_test".into()),
                state: PatternState::Confirmed,
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
                score: 0.9,
                weight: 1.0,
            }],
            confidence,
            validated_at: Utc::now(),
        }
    }

    fn cfg() -> AiStrategyProviderConfig {
        AiStrategyProviderConfig {
            min_validator_confidence: 0.7,
            min_ai_conviction: 0.6,
            risk_pct: dec!(0.005),
            time_in_force: TimeInForce::Gtc,
            time_stop_secs: Some(3600),
        }
    }

    fn ctx() -> StrategyContext {
        StrategyContext { run_mode: ExecutionMode::Dry }
    }

    #[test]
    fn act_verdict_above_floors_emits_intent() {
        let advisor = Arc::new(StubAdvisor(AiStrategyVerdict {
            act: true,
            conviction: 0.9,
            rationale: "looks great".into(),
        }));
        let p = AiStrategyProvider::new("ai_test", cfg(), advisor).unwrap();
        let intents = p.evaluate(&signal(0.85), &ctx()).unwrap();
        assert_eq!(intents.len(), 1);
        // Conviction is the floor of the two confidences.
        assert!((intents[0].conviction - 0.85).abs() < 1e-6);
    }

    #[test]
    fn hold_verdict_passes() {
        let advisor = Arc::new(StubAdvisor(AiStrategyVerdict {
            act: false,
            conviction: 0.9,
            rationale: "no thanks".into(),
        }));
        let p = AiStrategyProvider::new("ai_test", cfg(), advisor).unwrap();
        assert!(p.evaluate(&signal(0.85), &ctx()).unwrap().is_empty());
    }

    #[test]
    fn validator_floor_short_circuits_advisor() {
        let advisor = Arc::new(StubAdvisor(AiStrategyVerdict {
            act: true,
            conviction: 0.9,
            rationale: String::new(),
        }));
        let p = AiStrategyProvider::new("ai_test", cfg(), advisor).unwrap();
        assert!(p.evaluate(&signal(0.4), &ctx()).unwrap().is_empty());
    }

    #[test]
    fn advisor_failure_degrades_to_hold() {
        let p = AiStrategyProvider::new("ai_test", cfg(), Arc::new(FailingAdvisor)).unwrap();
        // Must NOT propagate the error — AI is never a hard dependency.
        assert!(p.evaluate(&signal(0.9), &ctx()).unwrap().is_empty());
    }
}
