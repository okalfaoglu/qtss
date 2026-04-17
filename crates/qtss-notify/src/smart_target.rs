//! Faz 9.7.4 — Smart Target AI (hybrid rule + LLM judge).
//!
//! When a `TpHit` transition fires, the watcher calls [`decide`]
//! instead of the pure 9.7.3 default `promote_tp_hit`. The dispatcher
//! picks an evaluator based on position health:
//!
//! * `health < llm_below`    → [`LlmJudge::evaluate`] (async)
//! * `health < rule_below`   → [`rule_evaluate`]       (pure)
//! * otherwise               → default [`SmartTargetAction::Ride`]
//!
//! This matches the locked B3 decision: LLM only reaches the hot
//! path when the position is already in bad shape; healthy positions
//! follow the simple "ride" default; warm positions get a rule table.
//!
//! CLAUDE.md #1 — rule evaluator is a small dispatch table (no
//! if/else chain). #3 — no side-effects here; the watcher consumes
//! the returned [`SmartTargetDecision`].

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::health::{HealthBand, HealthScore};

const MODULE: &str = "notify";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartTargetAction {
    /// Keep the full position; let price work into the next TP.
    Ride,
    /// Take a partial off (TpPartial emitted by watcher).
    Scale,
    /// Close the remainder at this level (TpFinal emitted).
    Exit,
    /// Move SL tighter (entry or current price minus buffer). No
    /// partial taken.
    Tighten,
}

impl SmartTargetAction {
    pub fn code(self) -> &'static str {
        match self {
            Self::Ride => "ride",
            Self::Scale => "scale",
            Self::Exit => "exit",
            Self::Tighten => "tighten",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SmartTargetEvaluatorKind {
    /// Default "ride" — health too good to care.
    DefaultRide,
    /// Deterministic rule table.
    Rule,
    /// LLM-backed judge (async, external).
    Llm,
}

/// Input to the evaluator — all the context the judge needs without
/// a DB call. Copy-cheap.
#[derive(Debug, Clone)]
pub struct SmartTargetInput {
    pub tp_index: u8,        // 1..=3 (bounded by card schema)
    pub total_tps: u8,       // how many TPs are defined on this setup
    pub health: HealthScore,
    pub price: Decimal,
    pub pnl_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartTargetDecision {
    pub action: SmartTargetAction,
    pub confidence: f64,   // [0,1]
    pub reasoning: String, // short phrase, user-safe
}

#[derive(Debug, Clone, Copy)]
pub struct SmartTargetCfg {
    pub rule_below: f64,
    pub llm_below: f64,
}

impl SmartTargetCfg {
    pub const FALLBACK: Self = Self { rule_below: 50.0, llm_below: 30.0 };
}

// ---------------------------------------------------------------------------
// Rule evaluator — pure, dispatch table keyed by (tp_index, band)
// ---------------------------------------------------------------------------

/// Deterministic rule table. Keyed by band; per-band picks an action
/// based on whether we're at the last TP or an earlier one.
pub fn rule_evaluate(input: &SmartTargetInput) -> SmartTargetDecision {
    let is_last_tp = input.tp_index >= input.total_tps;
    let (action, reason): (SmartTargetAction, &'static str) = match (input.health.band, is_last_tp)
    {
        (HealthBand::Healthy, _) => (
            SmartTargetAction::Ride,
            "Pozisyon sağlıklı, hedef doğrultusunda seyrediyor.",
        ),
        (HealthBand::Warn, false) => (
            SmartTargetAction::Scale,
            "Sağlık düşüyor — kısmi kâr alıp kalanı stopsuz bırakma.",
        ),
        (HealthBand::Warn, true) => (
            SmartTargetAction::Exit,
            "Son hedefe ulaştık ve sağlık zayıf — tüm kâr realize.",
        ),
        (HealthBand::Danger, false) => (
            SmartTargetAction::Tighten,
            "Piyasa kötüleşiyor — SL'i sıkılaştır, kârı koru.",
        ),
        (HealthBand::Danger, true) => (
            SmartTargetAction::Exit,
            "Son hedef + tehlike bandı: pozisyonu kapat.",
        ),
        (HealthBand::Critical, _) => (
            SmartTargetAction::Exit,
            "Kritik sağlık bandı — pozisyonu derhal kapat.",
        ),
    };
    SmartTargetDecision {
        action,
        confidence: match input.health.band {
            HealthBand::Healthy => 0.90,
            HealthBand::Warn => 0.80,
            HealthBand::Danger => 0.85,
            HealthBand::Critical => 0.95,
        },
        reasoning: reason.to_string(),
    }
}

// ---------------------------------------------------------------------------
// LLM judge trait + default stub
// ---------------------------------------------------------------------------

/// Async trait — concrete impls plug in Claude/Gemini/Ollama via the
/// existing `llm_judge` infrastructure. The default impl falls back
/// to the rule evaluator so the pipeline never blocks on a missing
/// LLM backend.
#[async_trait]
pub trait LlmJudge: Send + Sync {
    async fn evaluate(&self, input: &SmartTargetInput) -> SmartTargetDecision;
    fn name(&self) -> &'static str;
}

/// Default stand-in: runs the rule table, tagging the reasoning so
/// operators can see the LLM wasn't actually consulted yet.
pub struct DefaultLlmJudge;

#[async_trait]
impl LlmJudge for DefaultLlmJudge {
    fn name(&self) -> &'static str {
        "default_rule_fallback"
    }
    async fn evaluate(&self, input: &SmartTargetInput) -> SmartTargetDecision {
        let mut d = rule_evaluate(input);
        d.reasoning = format!("[LLM stub → kural] {}", d.reasoning);
        d
    }
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/// Entry point called by the watcher on every `TpHit`. Pure-async
/// signature so callers can await the LLM path.
pub async fn decide<J: LlmJudge + ?Sized>(
    input: &SmartTargetInput,
    cfg: &SmartTargetCfg,
    llm: &J,
) -> (SmartTargetDecision, SmartTargetEvaluatorKind) {
    let h = input.health.total;
    if h < cfg.llm_below {
        let d = llm.evaluate(input).await;
        return (d, SmartTargetEvaluatorKind::Llm);
    }
    if h < cfg.rule_below {
        return (rule_evaluate(input), SmartTargetEvaluatorKind::Rule);
    }
    (
        SmartTargetDecision {
            action: SmartTargetAction::Ride,
            confidence: 0.60,
            reasoning: "Sağlık yüksek — hedefe devam.".to_string(),
        },
        SmartTargetEvaluatorKind::DefaultRide,
    )
}

pub async fn load_config(pool: &PgPool) -> SmartTargetCfg {
    let f = SmartTargetCfg::FALLBACK;
    SmartTargetCfg {
        rule_below: qtss_storage::resolve_system_f64(
            pool, MODULE, "health.smart_target.rule_below",
            "QTSS_SMART_TARGET_RULE_BELOW", f.rule_below,
        )
        .await,
        llm_below: qtss_storage::resolve_system_f64(
            pool, MODULE, "health.smart_target.llm_below",
            "QTSS_SMART_TARGET_LLM_BELOW", f.llm_below,
        )
        .await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::{HealthBand, HealthComponents, HealthScore};
    use rust_decimal_macros::dec;

    fn health_at(band: HealthBand, total: f64) -> HealthScore {
        HealthScore { total, band, components: HealthComponents::default() }
    }

    fn input(tp_index: u8, total_tps: u8, band: HealthBand, total: f64) -> SmartTargetInput {
        SmartTargetInput {
            tp_index,
            total_tps,
            health: health_at(band, total),
            price: dec!(100),
            pnl_pct: Some(2.0),
        }
    }

    #[test]
    fn rule_healthy_rides_anywhere() {
        let d = rule_evaluate(&input(1, 3, HealthBand::Healthy, 80.0));
        assert_eq!(d.action, SmartTargetAction::Ride);
    }

    #[test]
    fn rule_warn_partial_scales_early_exits_final() {
        let d = rule_evaluate(&input(1, 3, HealthBand::Warn, 60.0));
        assert_eq!(d.action, SmartTargetAction::Scale);
        let d = rule_evaluate(&input(3, 3, HealthBand::Warn, 60.0));
        assert_eq!(d.action, SmartTargetAction::Exit);
    }

    #[test]
    fn rule_danger_tightens_then_exits_final() {
        let d = rule_evaluate(&input(2, 3, HealthBand::Danger, 35.0));
        assert_eq!(d.action, SmartTargetAction::Tighten);
        let d = rule_evaluate(&input(3, 3, HealthBand::Danger, 35.0));
        assert_eq!(d.action, SmartTargetAction::Exit);
    }

    #[test]
    fn rule_critical_always_exits() {
        for tp in 1..=3 {
            let d = rule_evaluate(&input(tp, 3, HealthBand::Critical, 15.0));
            assert_eq!(d.action, SmartTargetAction::Exit);
        }
    }

    #[tokio::test]
    async fn dispatch_picks_evaluator_by_health() {
        let cfg = SmartTargetCfg::FALLBACK; // rule_below=50, llm_below=30
        let llm = DefaultLlmJudge;

        // Healthy → default ride.
        let i = input(1, 3, HealthBand::Healthy, 80.0);
        let (d, k) = decide(&i, &cfg, &llm).await;
        assert_eq!(d.action, SmartTargetAction::Ride);
        assert!(matches!(k, SmartTargetEvaluatorKind::DefaultRide));

        // Between llm_below and rule_below → Rule.
        let i = input(1, 3, HealthBand::Warn, 45.0);
        let (_d, k) = decide(&i, &cfg, &llm).await;
        assert!(matches!(k, SmartTargetEvaluatorKind::Rule));

        // Below llm_below → LLM.
        let i = input(1, 3, HealthBand::Danger, 20.0);
        let (_d, k) = decide(&i, &cfg, &llm).await;
        assert!(matches!(k, SmartTargetEvaluatorKind::Llm));
    }
}
