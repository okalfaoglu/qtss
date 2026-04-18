//! Faz 9.7.5 — Real LLM-backed [`LlmJudge`] for Smart Target decisions.
//!
//! Wraps an [`AiCompletionProvider`] (any of the existing multi-vendor
//! backends: Anthropic / OpenAI-compatible / Ollama / Gemini) and
//! produces a [`SmartTargetDecision`] when position health drops into
//! the LLM band. On any failure (provider missing, network, malformed
//! JSON) we fall back to the deterministic rule table so the watcher
//! never blocks on a misbehaving model.
//!
//! CLAUDE.md #1 — no if/else chain: action parsing goes through a
//! static lookup table. #2 — provider / model / tokens are read from
//! `system_config` via [`AiEngineConfig`] (no hard-coded constants).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, warn};

use qtss_notify::{
    rule_evaluate, LlmJudge, SmartTargetAction, SmartTargetDecision, SmartTargetInput,
};

use crate::config::AiEngineConfig;
use crate::providers::{provider_for_layer, AiCompletionProvider, AiRequest, LayerKind};
use qtss_storage::AppConfigRepository;

/// Concrete [`LlmJudge`] that routes through the shared AI provider
/// stack. Use [`try_build`] to construct — returns `None` when the
/// engine is disabled or the tactical layer is not configured.
pub struct SmartTargetLlmJudge {
    provider: Arc<dyn AiCompletionProvider>,
    model: String,
    max_tokens: u32,
    temperature: f32,
}

impl SmartTargetLlmJudge {
    /// Preferred constructor: pulls `ai_engine_config` from the DB,
    /// picks the *tactical* provider (cheapest / lowest latency), and
    /// returns `None` when nothing sensible is wired up.
    pub async fn try_build(pool: &PgPool) -> Option<Arc<dyn LlmJudge>> {
        let repo = AppConfigRepository::new(pool.clone());
        let mut cfg = match repo.get_by_key("ai_engine_config").await.ok().flatten() {
            Some(row) => serde_json::from_value::<AiEngineConfig>(row.value)
                .unwrap_or_else(|_| AiEngineConfig::default_disabled()),
            None => AiEngineConfig::default_disabled(),
        };
        cfg.merge_env_overrides();
        if !cfg.enabled || !cfg.tactical_layer_enabled {
            debug!("SmartTargetLlmJudge: AI engine disabled → stub fallback");
            return None;
        }
        let secrets = crate::provider_secrets::AiProviderSecrets::load(pool).await;
        let provider = match provider_for_layer(&cfg, LayerKind::Tactical, &secrets) {
            Ok(p) => p,
            Err(e) => {
                warn!(%e, "SmartTargetLlmJudge: tactical provider unavailable");
                return None;
            }
        };
        Some(Arc::new(Self {
            provider,
            model: cfg.model_tactical.clone(),
            max_tokens: cfg.max_tokens_tactical.max(128),
            // Low-temperature: we want a terse JSON verdict, not creative prose.
            temperature: 0.2,
        }))
    }
}

// ---------------------------------------------------------------------------
// LlmJudge impl
// ---------------------------------------------------------------------------

#[async_trait]
impl LlmJudge for SmartTargetLlmJudge {
    fn name(&self) -> &'static str {
        "smart_target_llm"
    }

    async fn evaluate(&self, input: &SmartTargetInput) -> SmartTargetDecision {
        let system = Some(SYSTEM_PROMPT.to_string());
        let user = render_user_prompt(input);
        let req = AiRequest {
            system,
            user,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            model: self.model.clone(),
            force_json_mime: true,
        };
        match self.provider.complete(&req).await {
            Ok(resp) => parse_or_fallback(&resp.text, input),
            Err(e) => {
                warn!(provider=%self.provider.provider_id(), %e,
                      "SmartTargetLlmJudge: provider error → rule fallback");
                fallback(input, "LLM provider error, kural tablosu devrede.")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Prompt templates
// ---------------------------------------------------------------------------

const SYSTEM_PROMPT: &str = "Sen bir kripto pozisyon yönetim yargıcısın. \
Kullanıcı sana açık bir setup'ın sağlık skoru, TP ilerlemesi ve anlık PnL'sini verir. \
Cevabın **sadece** JSON olsun: \
{\"action\":\"ride|scale|exit|tighten|trail\",\"confidence\":0.0-1.0,\"reasoning\":\"<kısa türkçe\"}. \
ride: pozisyonu hedefe bırak. \
scale: kısmi kar al. \
exit: kalan pozisyonu kapat. \
tighten: stop'u sıkılaştır. \
trail: trailing stop moduna geç (genelde son TP yakınında, sağlık güçlüyse).";

fn render_user_prompt(input: &SmartTargetInput) -> String {
    let pnl = input
        .pnl_pct
        .map(|p| format!("{:+.2}%", p))
        .unwrap_or_else(|| "bilinmiyor".into());
    json!({
        "tp_index": input.tp_index,
        "total_tps": input.total_tps,
        "is_last_tp": input.tp_index >= input.total_tps,
        "health_total": input.health.total,
        "health_band": format!("{:?}", input.health.band),
        "price": input.price.to_string(),
        "pnl_pct": pnl,
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// Response parser
// ---------------------------------------------------------------------------

fn parse_or_fallback(text: &str, input: &SmartTargetInput) -> SmartTargetDecision {
    match try_parse(text) {
        Some(d) => d,
        None => {
            warn!(raw=%text.chars().take(240).collect::<String>(),
                  "SmartTargetLlmJudge: malformed JSON → rule fallback");
            fallback(input, "LLM cevabı çözümlenemedi, kural tablosu devrede.")
        }
    }
}

fn try_parse(text: &str) -> Option<SmartTargetDecision> {
    // Models occasionally wrap JSON in prose or code fences. Extract the
    // first balanced object we can find.
    let s = extract_json_object(text)?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    let action_str = v.get("action")?.as_str()?.trim().to_ascii_lowercase();
    let action = ACTION_TABLE
        .iter()
        .find(|(k, _)| *k == action_str.as_str())
        .map(|(_, a)| *a)?;
    let confidence = v
        .get("confidence")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    let reasoning = v
        .get("reasoning")
        .and_then(|x| x.as_str())
        .unwrap_or("LLM yargısı")
        .trim()
        .chars()
        .take(400)
        .collect::<String>();
    Some(SmartTargetDecision { action, confidence, reasoning })
}

fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth: i32 = 0;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// CLAUDE.md #1 — static lookup instead of nested match.
const ACTION_TABLE: &[(&str, SmartTargetAction)] = &[
    ("ride", SmartTargetAction::Ride),
    ("scale", SmartTargetAction::Scale),
    ("exit", SmartTargetAction::Exit),
    ("tighten", SmartTargetAction::Tighten),
    ("trail", SmartTargetAction::Trail),
];

fn fallback(input: &SmartTargetInput, prefix: &str) -> SmartTargetDecision {
    let mut d = rule_evaluate(input);
    d.reasoning = format!("[{}] {}", prefix, d.reasoning);
    d
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use qtss_notify::health::{HealthBand, HealthScore};
    use rust_decimal_macros::dec;

    fn sample_input() -> SmartTargetInput {
        SmartTargetInput {
            tp_index: 2,
            total_tps: 3,
            health: HealthScore { total: 42.0, band: HealthBand::Warn, components: Default::default() },
            price: dec!(100),
            pnl_pct: Some(1.8),
        }
    }

    #[test]
    fn extract_json_handles_prose_wrapper() {
        let raw = "Here you go:\n```json\n{\"action\":\"trail\",\"confidence\":0.82,\"reasoning\":\"son hedefe yakın\"}\n```";
        let obj = extract_json_object(raw).unwrap();
        let v: serde_json::Value = serde_json::from_str(&obj).unwrap();
        assert_eq!(v["action"], "trail");
    }

    #[test]
    fn try_parse_maps_actions() {
        let d = try_parse(r#"{"action":"exit","confidence":0.9,"reasoning":"r"}"#).unwrap();
        assert!(matches!(d.action, SmartTargetAction::Exit));
        assert!((d.confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn try_parse_rejects_unknown_action() {
        assert!(try_parse(r#"{"action":"panic","confidence":1,"reasoning":""}"#).is_none());
    }

    #[test]
    fn parse_or_fallback_returns_rule_on_garbage() {
        let d = parse_or_fallback("not even close to json", &sample_input());
        assert!(d.reasoning.starts_with('['));
    }
}
