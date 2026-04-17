//! Faz 9.5 — LLM Tiebreaker.
//!
//! When the LightGBM model scores a setup in the "uncertain zone"
//! (configurable, default 0.45–0.55), the system optionally consults
//! an LLM for a second opinion. The LLM sees the setup context
//! (symbol, TF, detection family/subkind, key indicators, SHAP top-5,
//! regime) and renders a verdict: pass / block / abstain with a
//! confidence and short reasoning text.
//!
//! Soft-fail everywhere: any error returns `None`, letting the
//! classic gate path proceed unimpeded.
//!
//! CLAUDE.md:
//!   * #1 — provider dispatch via match, each arm delegates to a fn.
//!   * #2 — all tunables from `system_config`; zero hardcoded values.
//!   * #3 — this is a pure HTTP adapter; knows nothing about setups.

use std::time::{Duration, Instant};

use qtss_storage::{resolve_system_f64, resolve_system_string, resolve_worker_enabled_flag};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::warn;

// Re-use the ShapEntry from ai_inference so the caller can pass them through.
pub use crate::ai_inference::ShapEntry;

/// Resolved tunables — looked up once per setup-open so Config GUI
/// edits apply without a worker restart (CLAUDE.md #2).
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub ollama_url: String,
    pub timeout_ms: u64,
    pub uncertain_lo: f64,
    pub uncertain_hi: f64,
    pub max_tokens: u64,
    pub prompt_version: String,
}

impl LlmConfig {
    pub async fn load(pool: &PgPool) -> Self {
        let enabled = resolve_worker_enabled_flag(
            pool, "ai", "llm.enabled", "", false,
        )
        .await;
        let provider = resolve_system_string(
            pool, "ai", "llm.provider", "", "claude",
        )
        .await;
        let model = resolve_system_string(
            pool, "ai", "llm.model", "", "claude-sonnet-4-20250514",
        )
        .await;
        let api_key = resolve_system_string(
            pool, "ai", "llm.api_key", "", "",
        )
        .await;
        let ollama_url = resolve_system_string(
            pool, "ai", "llm.ollama_url", "", "http://127.0.0.1:11434",
        )
        .await;
        let timeout_ms = resolve_system_f64(
            pool, "ai", "llm.timeout_ms", "", 10000.0,
        )
        .await as u64;
        let uncertain_lo = resolve_system_f64(
            pool, "ai", "llm.uncertain_lo", "", 0.45,
        )
        .await;
        let uncertain_hi = resolve_system_f64(
            pool, "ai", "llm.uncertain_hi", "", 0.55,
        )
        .await;
        let max_tokens = resolve_system_f64(
            pool, "ai", "llm.max_tokens", "", 256.0,
        )
        .await as u64;
        let prompt_version = resolve_system_string(
            pool, "ai", "llm.prompt_version", "", "v1",
        )
        .await;

        Self {
            enabled,
            provider,
            model,
            api_key,
            ollama_url,
            timeout_ms,
            uncertain_lo,
            uncertain_hi,
            max_tokens,
            prompt_version,
        }
    }

    pub fn score_in_uncertain_zone(&self, score: f64) -> bool {
        score >= self.uncertain_lo && score <= self.uncertain_hi
    }
}

/// Context the LLM sees when rendering a verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupContext {
    pub symbol: String,
    pub timeframe: String,
    pub family: String,
    pub subkind: String,
    pub ai_score: f64,
    pub shap_top5: Vec<ShapEntry>,
    pub regime: String,
    pub structural_score: f64,
    pub confidence: f64,
}

/// Parsed LLM verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmVerdict {
    pub verdict: String,
    pub confidence: f64,
    pub reasoning: String,
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub latency_ms: i64,
    pub raw_response: serde_json::Value,
}

/// Main entry point — soft-fail, returns `None` on any error.
pub async fn judge(cfg: &LlmConfig, ctx: &SetupContext) -> Option<LlmVerdict> {
    if !cfg.enabled || cfg.api_key.is_empty() {
        return None;
    }
    let result = match cfg.provider.as_str() {
        "claude" => judge_claude(cfg, ctx).await,
        "gemini" => judge_gemini(cfg, ctx).await,
        "ollama" => judge_ollama(cfg, ctx).await,
        other => {
            warn!(provider = %other, "unknown LLM provider");
            return None;
        }
    };
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            warn!(error = %e, provider = %cfg.provider, "LLM judge failed (soft-fail)");
            None
        }
    }
}

// ─── prompt template ─────────────────────────────────────────────

fn build_prompt(ctx: &SetupContext, version: &str, lo: f64, hi: f64) -> String {
    let shap_lines: String = ctx
        .shap_top5
        .iter()
        .map(|s| format!("  - {}: value={:.4}, contribution={:+.4}", s.feature, s.value, s.contribution))
        .collect::<Vec<_>>()
        .join("\n");

    match version {
        _ => format!(
            r#"You are a quantitative trading analyst reviewing a potential trade setup.

Setup context:
- Symbol: {symbol}, Timeframe: {timeframe}
- Pattern: {family}/{subkind}
- AI model score: {ai_score:.3} (uncertain zone: {lo:.2}–{hi:.2})
- Structural score: {structural:.2}, Detection confidence: {conf:.2}
- Market regime: {regime}
- Top contributing features (SHAP):
{shap}

The AI model score falls in the uncertain zone. Based on the above context, should this setup proceed to execution?

Respond with EXACTLY this JSON format:
{{"verdict": "pass"|"block"|"abstain", "confidence": 0.0-1.0, "reasoning": "<1-2 sentences>"}}

Guidelines:
- "pass" = proceed with the trade
- "block" = skip this trade
- "abstain" = insufficient info to decide (falls back to classic gate)
- Consider regime alignment, pattern reliability, and feature contributions
- Be concise in reasoning"#,
            symbol = ctx.symbol,
            timeframe = ctx.timeframe,
            family = ctx.family,
            subkind = ctx.subkind,
            ai_score = ctx.ai_score,
            lo = lo,
            hi = hi,
            structural = ctx.structural_score,
            conf = ctx.confidence,
            regime = ctx.regime,
            shap = shap_lines,
        ),
    }
}

// ─── provider implementations ────────────────────────────────────

type JudgeResult = Result<LlmVerdict, Box<dyn std::error::Error + Send + Sync>>;

async fn judge_claude(cfg: &LlmConfig, ctx: &SetupContext) -> JudgeResult {
    let client = reqwest::Client::new();
    let prompt = build_prompt(ctx, &cfg.prompt_version, cfg.uncertain_lo, cfg.uncertain_hi);
    let start = Instant::now();

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &cfg.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .timeout(Duration::from_millis(cfg.timeout_ms))
        .json(&serde_json::json!({
            "model": cfg.model,
            "max_tokens": cfg.max_tokens,
            "messages": [{"role": "user", "content": prompt}]
        }))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let latency_ms = start.elapsed().as_millis() as i64;
    let text = resp["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let input_tokens = resp["usage"]["input_tokens"].as_i64().map(|v| v as i32);
    let output_tokens = resp["usage"]["output_tokens"].as_i64().map(|v| v as i32);

    parse_llm_response(text, input_tokens, output_tokens, latency_ms, resp)
}

async fn judge_gemini(cfg: &LlmConfig, ctx: &SetupContext) -> JudgeResult {
    let client = reqwest::Client::new();
    let prompt = build_prompt(ctx, &cfg.prompt_version, cfg.uncertain_lo, cfg.uncertain_hi);
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        cfg.model, cfg.api_key,
    );
    let start = Instant::now();

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .timeout(Duration::from_millis(cfg.timeout_ms))
        .json(&serde_json::json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {"maxOutputTokens": cfg.max_tokens}
        }))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let latency_ms = start.elapsed().as_millis() as i64;
    let text = resp["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let input_tokens = resp["usageMetadata"]["promptTokenCount"].as_i64().map(|v| v as i32);
    let output_tokens = resp["usageMetadata"]["candidatesTokenCount"].as_i64().map(|v| v as i32);

    parse_llm_response(text, input_tokens, output_tokens, latency_ms, resp)
}

async fn judge_ollama(cfg: &LlmConfig, ctx: &SetupContext) -> JudgeResult {
    let client = reqwest::Client::new();
    let prompt = build_prompt(ctx, &cfg.prompt_version, cfg.uncertain_lo, cfg.uncertain_hi);
    let url = format!("{}/api/generate", cfg.ollama_url);
    let start = Instant::now();

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .timeout(Duration::from_millis(cfg.timeout_ms))
        .json(&serde_json::json!({
            "model": cfg.model,
            "prompt": prompt,
            "stream": false
        }))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let latency_ms = start.elapsed().as_millis() as i64;
    let text = resp["response"]
        .as_str()
        .unwrap_or("")
        .to_string();
    // Ollama doesn't always report token counts; extract if present.
    let input_tokens = resp["prompt_eval_count"].as_i64().map(|v| v as i32);
    let output_tokens = resp["eval_count"].as_i64().map(|v| v as i32);

    parse_llm_response(text, input_tokens, output_tokens, latency_ms, resp)
}

// ─── response parser ─────────────────────────────────────────────

/// Extract JSON from the LLM text (may be wrapped in markdown fences),
/// validate verdict, and produce a `LlmVerdict`.
fn parse_llm_response(
    text: String,
    input_tokens: Option<i32>,
    output_tokens: Option<i32>,
    latency_ms: i64,
    raw: serde_json::Value,
) -> JudgeResult {
    // Strip optional markdown code fences.
    let trimmed = text.trim();
    let json_str = if trimmed.starts_with("```") {
        trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };

    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("failed to parse LLM JSON: {e} — raw: {json_str}"))?;

    let verdict = parsed["verdict"]
        .as_str()
        .unwrap_or("abstain")
        .to_string();

    // Validate verdict is one of the three accepted values.
    if !matches!(verdict.as_str(), "pass" | "block" | "abstain") {
        return Err(format!("invalid verdict: {verdict}").into());
    }

    let confidence = parsed["confidence"]
        .as_f64()
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);

    let reasoning = parsed["reasoning"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(LlmVerdict {
        verdict,
        confidence,
        reasoning,
        input_tokens,
        output_tokens,
        latency_ms,
        raw_response: raw,
    })
}
