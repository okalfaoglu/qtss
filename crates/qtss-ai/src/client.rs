//! `AiRuntime` — pooled DB handle, merged config, and per-layer providers (optional per layer).

use std::sync::Arc;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::approval::maybe_auto_approve;
use crate::config::AiEngineConfig;
use crate::error::AiResult;
use crate::parser::{parse_operational_decision, parse_portfolio_decision, parse_tactical_decision};
use crate::providers::{AiCompletionProvider, AiRequest, LayerKind};
use crate::providers;
use crate::safety::{SafetyConfig, validate_ai_decision_safety};
use crate::storage::{
    decision_exists_for_hash, expire_stale_decisions, insert_ai_decision, insert_ai_decision_error,
    insert_portfolio_directive, insert_position_directive, insert_tactical_decision,
};
use qtss_notify::NotificationDispatcher;
use qtss_storage::{
    list_enabled_engine_symbols, symbols_with_positive_long_from_fills, AppConfigRepository,
    ExchangeOrderRepository,
};

/// Scales `position_size_multiplier` by strategic `portfolio_directive.symbol_weight_0_1` when present (`QTSS_MASTER_DEV_GUIDE` FAZ 6.2).
fn apply_strategic_portfolio_weight(ctx: &Value, parsed: &mut Value) {
    let Some(w) = ctx
        .get("portfolio_directive")
        .filter(|p| !p.is_null())
        .and_then(|p| p.get("symbol_weight_0_1"))
        .and_then(|x| x.as_f64())
    else {
        return;
    };
    if !w.is_finite() || w <= 0.0 {
        return;
    }
    let w = w.clamp(0.0, 1.0);
    let base = parsed
        .get("position_size_multiplier")
        .and_then(|x| x.as_f64())
        .unwrap_or(1.0);
    let adj = (base * w).clamp(0.0, 2.0);
    if let Some(obj) = parsed.as_object_mut() {
        obj.insert("position_size_multiplier".into(), json!(adj));
    }
}

fn connect_optional_provider(
    cfg: &AiEngineConfig,
    layer: LayerKind,
    layer_on: bool,
    secrets: &crate::provider_secrets::AiProviderSecrets,
) -> Option<Arc<dyn AiCompletionProvider>> {
    if !cfg.enabled || !layer_on {
        return None;
    }
    match providers::provider_for_layer(cfg, layer, secrets) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!(
                ?layer,
                error = %e,
                "AI provider not available for layer — skipping this layer"
            );
            None
        }
    }
}

/// Full AI runtime: per-layer providers are [`None`] when misconfigured (worker continues).
pub struct AiRuntime {
    pool: PgPool,
    config: AiEngineConfig,
    tactical_provider: Option<Arc<dyn AiCompletionProvider>>,
    operational_provider: Option<Arc<dyn AiCompletionProvider>>,
    strategic_provider: Option<Arc<dyn AiCompletionProvider>>,
    notify: Option<NotificationDispatcher>,
}

impl AiRuntime {
    pub async fn from_pool(pool: PgPool) -> AiResult<Self> {
        let secrets = crate::provider_secrets::AiProviderSecrets::load(&pool).await;
        let repo = AppConfigRepository::new(pool.clone());
        let mut config = match repo.get_by_key("ai_engine_config").await? {
            Some(row) => serde_json::from_value(row.value).unwrap_or_else(|_| AiEngineConfig::default_disabled()),
            None => AiEngineConfig::default_disabled(),
        };
        config.merge_env_overrides();
        let tactical_provider = connect_optional_provider(
            &config,
            LayerKind::Tactical,
            config.tactical_layer_enabled,
            &secrets,
        );
        let operational_provider = connect_optional_provider(
            &config,
            LayerKind::Operational,
            config.operational_layer_enabled,
            &secrets,
        );
        let strategic_provider = connect_optional_provider(
            &config,
            LayerKind::Strategic,
            config.strategic_layer_enabled,
            &secrets,
        );
        let ncfg = crate::notify_telegram_config::load_notify_config_merged(&pool).await;
        let notify = NotificationDispatcher::new(ncfg);
        let notify = if notify.config().telegram.is_some() || notify.config().webhook.is_some() {
            Some(notify)
        } else {
            None
        };
        Ok(Self {
            pool,
            config,
            tactical_provider,
            operational_provider,
            strategic_provider,
            notify,
        })
    }

    #[inline]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    #[inline]
    pub fn config(&self) -> &AiEngineConfig {
        &self.config
    }

    #[inline]
    pub fn tactical_provider(&self) -> Option<&Arc<dyn AiCompletionProvider>> {
        self.tactical_provider.as_ref()
    }

    #[inline]
    pub fn operational_provider(&self) -> Option<&Arc<dyn AiCompletionProvider>> {
        self.operational_provider.as_ref()
    }

    #[inline]
    pub fn strategic_provider(&self) -> Option<&Arc<dyn AiCompletionProvider>> {
        self.strategic_provider.as_ref()
    }

    #[inline]
    pub fn notify_dispatcher(&self) -> Option<&NotificationDispatcher> {
        self.notify.as_ref()
    }
}

/// Stable hash for deduplicating identical contexts (SHA‑256 hex).
pub fn hash_context(snapshot: &Value) -> String {
    let bytes = serde_json::to_vec(snapshot).unwrap_or_default();
    let out = Sha256::digest(&bytes);
    hex::encode(out)
}

fn tactical_system_prompt(cfg: &AiEngineConfig) -> String {
    let locale = cfg.output_locale.as_deref().unwrap_or("en");
    let reasoning_lang = if locale.to_lowercase().starts_with("tr") {
        "`reasoning` alanını Türkçe ve kısa yaz."
    } else {
        "Keep `reasoning` short in the requested locale."
    };
    let criteria = if locale.to_lowercase().starts_with("tr") {
        r#"Kurallar: onchain aggregate_score > 0.6 ve conflict yok → buy/strong_buy eğilimi; < -0.6 ve conflict yok → sell/strong_sell; conflict var → position_size_multiplier ≤ 0.5 veya no_trade; açık pozisyon + aynı yön → genelde no_trade; confidence < 0.5 → no_trade."#
    } else {
        r#"Rules: aggregate_score > 0.6 without conflict → buy bias; < -0.6 without conflict → sell bias; on conflict → multiplier ≤ 0.5 or no_trade; existing position same direction → prefer no_trade; confidence < 0.5 → no_trade."#
    };
    format!(
        r#"You are a tactical trading advisor for QTSS. Reply with a single JSON object only (no markdown fences).
Locale: {locale}. {reasoning_lang}
Required keys: "direction" (strong_buy|buy|neutral|sell|strong_sell|no_trade), "confidence" (0.0-1.0).
Directional trades (not neutral/no_trade) MUST include positive "stop_loss_pct" (percent, > 0).
Optional: "position_size_multiplier" (0.0-2.0), "take_profit_pct", "entry_price_hint", "reasoning".
Context may include `portfolio_directive` from the strategic layer: honor `symbol_weight_0_1`, `preferred_regime`, and `risk_budget_pct` when deciding direction and size.
{criteria}
Temperature: conservative; output JSON only."#
    )
}

fn operational_system_prompt(cfg: &AiEngineConfig) -> String {
    let locale = cfg.output_locale.as_deref().unwrap_or("en");
    format!(
        r#"You manage open positions for QTSS (operational layer). Reply JSON only, locale {locale}.
Required: "action" one of: keep, tighten_stop, widen_stop, activate_trailing, deactivate_trailing, partial_close, full_close, add_to_position.
Optional: new_stop_loss_pct, new_take_profit_pct, trailing_callback_pct, partial_close_pct, reasoning.
Never loosen stops without justification; prefer protecting capital."#
    )
}

fn strategic_system_prompt(cfg: &AiEngineConfig) -> String {
    let locale = cfg.output_locale.as_deref().unwrap_or("en");
    format!(
        r#"You are the strategic portfolio advisor for QTSS. Reply JSON only, locale {locale}.
Include when possible: "risk_budget_pct", "max_open_positions", "preferred_regime", "symbol_scores" (object symbol→weight 0-1), "macro_note".
No prose outside JSON."#
    )
}

/// One full tactical sweep over enabled engine symbols (FAZ 4.2 / 5).
pub async fn run_tactical_sweep(rt: &AiRuntime) -> AiResult<()> {
    expire_stale_decisions(rt.pool()).await?;
    if !rt.config().enabled || !rt.config().tactical_layer_enabled {
        return Ok(());
    }
    let Some(provider) = rt.tactical_provider() else {
        return Ok(());
    };
    let safety = SafetyConfig::from_ai_engine_config(rt.config());
    let symbols = list_enabled_engine_symbols(rt.pool()).await?;
    for e in symbols {
        let sym = e.symbol.clone();
        let ctx = match crate::context_builder::build_tactical_context(rt.pool(), &sym).await {
            Ok(c) => c,
            Err(err) => {
                tracing::warn!(?err, %sym, "tactical context build failed");
                continue;
            }
        };
        let h = hash_context(&ctx);
        if decision_exists_for_hash(rt.pool(), &h, 30).await? {
            continue;
        }
        let user = serde_json::to_string_pretty(&ctx).unwrap_or_else(|_| "{}".to_string());
        let req = AiRequest {
            system: Some(tactical_system_prompt(rt.config())),
            user,
            max_tokens: rt.config().max_tokens_tactical,
            temperature: 0.3,
            model: rt.config().model_tactical.clone(),
        };
        let resp = match provider.complete(&req).await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(?err, %sym, "tactical LLM call failed");
                let _ = insert_ai_decision_error(
                    rt.pool(),
                    "tactical",
                    Some(&sym),
                    &ctx,
                    &format!("provider_error: {err}"),
                    &json!({ "provider": provider.provider_id() }),
                )
                .await;
                continue;
            }
        };
        let mut parsed = match parse_tactical_decision(&resp.text) {
            Ok(p) => p,
            Err(err) => {
                let _ = insert_ai_decision_error(
                    rt.pool(),
                    "tactical",
                    Some(&sym),
                    &ctx,
                    &format!("parse_error: {err}; raw_head={}", &resp.text.chars().take(400).collect::<String>()),
                    &json!({
                        "provider": resp.provider_id,
                        "model": resp.model,
                    }),
                )
                .await;
                continue;
            }
        };
        apply_strategic_portfolio_weight(&ctx, &mut parsed);
        let direction = parsed
            .get("direction")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if direction == "no_trade" {
            tracing::info!(%sym, "tactical LLM returned no_trade; skipping persistence");
            continue;
        }
        let confidence = parsed.get("confidence").and_then(|x| x.as_f64()).unwrap_or(0.0);
        if confidence + f64::EPSILON < rt.config().require_min_confidence {
            tracing::info!(
                %sym,
                confidence,
                min = rt.config().require_min_confidence,
                "below minimum confidence; skipping persistence"
            );
            continue;
        }
        if let Err(e) = validate_ai_decision_safety(&parsed, &safety) {
            tracing::warn!(%sym, err = e, "tactical decision failed safety");
            let _ = insert_ai_decision_error(
                rt.pool(),
                "tactical",
                Some(&sym),
                &ctx,
                &format!("safety: {e}"),
                &json!({ "parsed": parsed }),
            )
            .await;
            continue;
        }
        let valid_until = chrono::Utc::now()
            + chrono::Duration::seconds(rt.config().decision_ttl_secs as i64);
        let meta = json!({
            "provider": resp.provider_id,
            "model": resp.model,
            "endpoint_host_hint": std::env::var("QTSS_AI_OPENAI_COMPAT_BASE_URL").ok().as_deref()
                .map(|s| s.split("//").nth(1).unwrap_or(s).split('/').next().unwrap_or("")),
        });
        let decision_id = insert_ai_decision(
            rt.pool(),
            "tactical",
            Some(&sym),
            Some(&resp.model),
            Some(&h),
            &ctx,
            Some(&resp.text),
            Some(&parsed),
            Some(confidence),
            rt.config().decision_ttl_secs,
            None,
            &meta,
        )
        .await?;
        insert_tactical_decision(rt.pool(), decision_id, &sym, &parsed, valid_until).await?;
        maybe_auto_approve(
            rt.pool(),
            decision_id,
            confidence,
            rt.config(),
            rt.notify_dispatcher(),
            Some(&sym),
            Some(direction),
            parsed.get("reasoning").and_then(|x| x.as_str()),
        )
        .await?;
    }
    Ok(())
}

/// Operational sweep: symbols with estimated net long (FAZ 6.1).
pub async fn run_operational_sweep(rt: &AiRuntime) -> AiResult<()> {
    expire_stale_decisions(rt.pool()).await?;
    if !rt.config().enabled || !rt.config().operational_layer_enabled {
        return Ok(());
    }
    let Some(provider) = rt.operational_provider() else {
        return Ok(());
    };
    let repo = ExchangeOrderRepository::new(rt.pool().clone());
    let rows = repo.list_recent_filled_orders_global(2000).await?;
    use rust_decimal::Decimal;
    let min_q = Decimal::new(1, 8);
    let symbols = symbols_with_positive_long_from_fills(&rows, min_q);
    for sym in symbols {
        let ctx = match crate::context_builder::build_operational_context(rt.pool(), &sym).await {
            Ok(c) => c,
            Err(err) => {
                tracing::warn!(?err, %sym, "operational context failed");
                continue;
            }
        };
        let h = hash_context(&ctx);
        if decision_exists_for_hash(rt.pool(), &format!("op:{h}"), 15).await? {
            continue;
        }
        let user = serde_json::to_string_pretty(&ctx).unwrap_or_else(|_| "{}".to_string());
        let req = AiRequest {
            system: Some(operational_system_prompt(rt.config())),
            user,
            max_tokens: rt.config().max_tokens_operational,
            temperature: 0.2,
            model: rt.config().model_operational.clone(),
        };
        let resp = match provider.complete(&req).await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(?err, %sym, "operational LLM failed");
                continue;
            }
        };
        let parsed = match parse_operational_decision(&resp.text) {
            Ok(p) => p,
            Err(err) => {
                tracing::warn!(%sym, ?err, "operational parse failed");
                continue;
            }
        };
        let confidence = parsed.get("confidence").and_then(|x| x.as_f64()).unwrap_or(0.75);
        let meta = json!({ "provider": resp.provider_id, "model": resp.model });
        let decision_id = insert_ai_decision(
            rt.pool(),
            "operational",
            Some(&sym),
            Some(&resp.model),
            Some(&format!("op:{h}")),
            &ctx,
            Some(&resp.text),
            Some(&parsed),
            Some(confidence),
            rt.config().decision_ttl_secs.min(3600),
            None,
            &meta,
        )
        .await?;
        insert_position_directive(rt.pool(), decision_id, &sym, &parsed).await?;
        maybe_auto_approve(
            rt.pool(),
            decision_id,
            confidence,
            rt.config(),
            rt.notify_dispatcher(),
            Some(&sym),
            parsed.get("action").and_then(|x| x.as_str()),
            parsed.get("reasoning").and_then(|x| x.as_str()),
        )
        .await?;
    }
    Ok(())
}

/// Strategic sweep: portfolio directive (FAZ 6.2).
pub async fn run_strategic_sweep(rt: &AiRuntime) -> AiResult<()> {
    expire_stale_decisions(rt.pool()).await?;
    if !rt.config().enabled || !rt.config().strategic_layer_enabled {
        return Ok(());
    }
    let Some(provider) = rt.strategic_provider() else {
        return Ok(());
    };
    let ctx = crate::context_builder::build_strategic_context(rt.pool()).await?;
    let h = hash_context(&ctx);
    if decision_exists_for_hash(rt.pool(), &format!("st:{h}"), 60 * 24).await? {
        return Ok(());
    }
    let user = serde_json::to_string_pretty(&ctx).unwrap_or_else(|_| "{}".to_string());
    let req = AiRequest {
        system: Some(strategic_system_prompt(rt.config())),
        user,
        max_tokens: rt.config().max_tokens_strategic,
        temperature: 0.35,
        model: rt.config().model_strategic.clone(),
    };
    let resp = provider.complete(&req).await?;
    let parsed = match parse_portfolio_decision(&resp.text) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(%e, "strategic portfolio JSON parse failed");
            return Ok(());
        }
    };
    let confidence = parsed.get("confidence").and_then(|x| x.as_f64()).unwrap_or(0.7);
    let meta = json!({ "provider": resp.provider_id, "model": resp.model });
    let valid_until = chrono::Utc::now() + chrono::Duration::hours(24 * 7);
    let decision_id = insert_ai_decision(
        rt.pool(),
        "strategic",
        None,
        Some(&resp.model),
        Some(&format!("st:{h}")),
        &ctx,
        Some(&resp.text),
        Some(&parsed),
        Some(confidence),
        86400 * 7,
        None,
        &meta,
    )
    .await?;
    insert_portfolio_directive(rt.pool(), decision_id, &parsed, Some(valid_until)).await?;
    maybe_auto_approve(
        rt.pool(),
        decision_id,
        confidence,
        rt.config(),
        rt.notify_dispatcher(),
        None,
        None,
        parsed.get("macro_note").and_then(|x| x.as_str()),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod strategic_portfolio_weight_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn apply_strategic_portfolio_weight_scales_multiplier() {
        let mut parsed = json!({
            "direction": "buy",
            "confidence": 0.9,
            "stop_loss_pct": 2.0,
            "position_size_multiplier": 1.0
        });
        let ctx = json!({ "portfolio_directive": { "symbol_weight_0_1": 0.5 } });
        apply_strategic_portfolio_weight(&ctx, &mut parsed);
        assert!(
            (parsed["position_size_multiplier"].as_f64().unwrap() - 0.5).abs() < f64::EPSILON
        );
    }

    #[test]
    fn apply_strategic_portfolio_weight_no_field_unchanged() {
        let mut parsed = json!({
            "direction": "buy",
            "confidence": 0.9,
            "stop_loss_pct": 2.0,
            "position_size_multiplier": 1.2
        });
        let ctx = json!({ "portfolio_directive": serde_json::Value::Null });
        apply_strategic_portfolio_weight(&ctx, &mut parsed);
        assert!(
            (parsed["position_size_multiplier"].as_f64().unwrap() - 1.2).abs() < f64::EPSILON
        );
    }
}