//! `AiRuntime` — pooled DB handle, merged config, and per-layer providers (optional per layer).

use std::sync::Arc;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::approval::maybe_auto_approve;
use crate::circuit_breaker::CircuitBreaker;
use crate::config::AiEngineConfig;
use crate::error::AiResult;
use crate::parser::{parse_operational_decision, parse_portfolio_decision, parse_tactical_decision};
use crate::providers::{AiCompletionProvider, AiRequest, AiResponse, LayerKind};
use crate::providers;
use crate::safety::{SafetyConfig, validate_ai_decision_safety, validate_operational_decision_safety, validate_strategic_decision_safety};
use crate::storage::{
    decision_exists_for_hash, expire_stale_decisions, insert_ai_decision, insert_ai_decision_error,
    insert_portfolio_directive, insert_position_directive, insert_tactical_decision,
};
use qtss_notify::NotificationDispatcher;
use qtss_storage::{
    list_enabled_engine_symbols, symbols_with_open_positions_from_fills, AppConfigRepository,
    ExchangeOrderRepository,
};

/// Provider call with exponential backoff retry (max 3 attempts) and circuit breaker.
/// Retries on transient HTTP errors (timeout, 429, 5xx); non-retryable errors propagate immediately.
async fn complete_with_retry(
    provider: &dyn AiCompletionProvider,
    req: &AiRequest,
    breaker: &CircuitBreaker,
) -> crate::error::AiResult<AiResponse> {
    if !breaker.allow() {
        return Err(crate::error::AiError::http(format!(
            "circuit breaker open for provider {} ({} consecutive failures)",
            provider.provider_id(),
            breaker.consecutive_failures(),
        )));
    }

    const MAX_ATTEMPTS: u32 = 3;
    const BASE_DELAY_MS: u64 = 1000;

    let mut last_err = None;
    for attempt in 0..MAX_ATTEMPTS {
        match provider.complete(req).await {
            Ok(resp) => {
                breaker.record_success();
                return Ok(resp);
            }
            Err(e) => {
                let retryable = matches!(&e, crate::error::AiError::Http(msg) if
                    msg.contains("429") ||
                    msg.contains("500") ||
                    msg.contains("502") ||
                    msg.contains("503") ||
                    msg.contains("504") ||
                    msg.contains("timeout") ||
                    msg.contains("timed out") ||
                    msg.contains("connection")
                );
                if !retryable || attempt + 1 == MAX_ATTEMPTS {
                    breaker.record_failure();
                    return Err(e);
                }
                let delay = BASE_DELAY_MS * 2_u64.pow(attempt);
                tracing::warn!(
                    provider = provider.provider_id(),
                    attempt = attempt + 1,
                    delay_ms = delay,
                    error = %e,
                    "retryable LLM error, backing off"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                last_err = Some(e);
            }
        }
    }
    breaker.record_failure();
    Err(last_err.unwrap())
}

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
    tactical_breaker: Arc<CircuitBreaker>,
    operational_breaker: Arc<CircuitBreaker>,
    strategic_breaker: Arc<CircuitBreaker>,
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
            tactical_breaker: Arc::new(CircuitBreaker::new(5, 120)),
            operational_breaker: Arc::new(CircuitBreaker::new(5, 120)),
            strategic_breaker: Arc::new(CircuitBreaker::new(3, 300)),
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

    #[inline]
    pub fn tactical_breaker(&self) -> &CircuitBreaker {
        &self.tactical_breaker
    }

    #[inline]
    pub fn operational_breaker(&self) -> &CircuitBreaker {
        &self.operational_breaker
    }

    #[inline]
    pub fn strategic_breaker(&self) -> &CircuitBreaker {
        &self.strategic_breaker
    }
}

/// Stable hash for deduplicating identical contexts (SHA‑256 hex).
pub fn hash_context(snapshot: &Value) -> String {
    let bytes = serde_json::to_vec(snapshot).unwrap_or_default();
    let out = Sha256::digest(&bytes);
    hex::encode(out)
}

/// Resolve system prompt: DB override (`app_config` key) → hardcoded default.
async fn resolve_system_prompt(pool: &PgPool, config_key: &str, default_fn: impl FnOnce() -> String) -> String {
    let repo = AppConfigRepository::new(pool.clone());
    match repo.get_by_key(config_key).await {
        Ok(Some(row)) => {
            if let Some(s) = row.value.as_str() {
                if !s.trim().is_empty() {
                    return s.to_string();
                }
            }
            // value might be {"prompt": "..."} object
            if let Some(s) = row.value.get("prompt").and_then(|v| v.as_str()) {
                if !s.trim().is_empty() {
                    return s.to_string();
                }
            }
            default_fn()
        }
        _ => default_fn(),
    }
}

fn tactical_system_prompt_default(cfg: &AiEngineConfig) -> String {
    let locale = cfg.output_locale.as_deref().unwrap_or("en");
    let reasoning_lang = if locale.to_lowercase().starts_with("tr") {
        "`reasoning` isteğe bağlı; kullanırsan MUTLAKA son alan olsun, Türkçe, tek kısa cümle, en fazla 120 karakter — aksi halde çıktı token sınırında kesilir ve JSON geçersiz olur."
    } else {
        "Optional `reasoning` must be the LAST JSON key if present: one short phrase, max 120 characters in the requested locale — otherwise output may truncate mid-string and break JSON."
    };
    let criteria = if locale.to_lowercase().starts_with("tr") {
        r#"Kurallar: onchain aggregate_score > 0.6 ve conflict yok → buy/strong_buy eğilimi; < -0.6 ve conflict yok → sell/strong_sell; conflict var → position_size_multiplier ≤ 0.5 veya no_trade; açık pozisyon + aynı yön → genelde no_trade; confidence < 0.5 → no_trade."#
    } else {
        r#"Rules: aggregate_score > 0.6 without conflict → buy bias; < -0.6 without conflict → sell bias; on conflict → multiplier ≤ 0.5 or no_trade; existing position same direction → prefer no_trade; confidence < 0.5 → no_trade."#
    };
    format!(
        r#"You are a tactical trading advisor for QTSS. Reply with one JSON object only — raw JSON, no markdown fences (no ```json blocks).
Locale: {locale}. {reasoning_lang}
Required keys first: "direction" (strong_buy|buy|neutral|sell|strong_sell|no_trade), "confidence" (0.0-1.0).
Then if applicable: positive "stop_loss_pct" for directional trades (not neutral/no_trade), then optional "position_size_multiplier" (0.0-2.0), "take_profit_pct", "entry_price_hint".
Optional last: "reasoning" (omit entirely if unsure — prefer no reasoning over a long one).
Omit "position_size_multiplier", "take_profit_pct", and "entry_price_hint" unless they differ from defaults; never emit partial keys — saves tokens and avoids truncated JSON.
Context may include `portfolio_directive` from the strategic layer: honor `symbol_weight_0_1`, `preferred_regime`, and `risk_budget_pct` when deciding direction and size.
Context may include `decision_history` — your recent decisions for this symbol with outcomes. Avoid flip-flopping; if your last decision was recent and the context hasn't materially changed, prefer consistency. Learn from outcomes (profit/loss).
Context may include `ai_feedback` with past decision outcomes (win_rate, avg_pnl_pct): factor these into confidence calibration.
Context may include `chart_formations` with detected classic patterns (Double Top/Bottom, Head & Shoulders, Triple Top/Bottom, Flag). Each formation has: pattern_name, neckline, target_price, quality (0-1), volume_analysis (divergence, breakout confirmation). High quality formations with volume confirmation are strong signals. Bearish patterns (Double Top, H&S, Triple Top, Bearish Flag) → sell bias; bullish patterns (Double Bottom, Inverse H&S, Triple Bottom, Bullish Flag) → buy bias. Volume divergence strengthens the signal.
Context may include `tbm_scores` (Top/Bottom Mining) with reversal detection across 4 pillars: Momentum (Stochastic, MACD, EMA cross, divergence), Volume (MFI, OBV, CVD, volume spike), Structure (Fibonacci, Bollinger, squeeze, chart pattern), Onchain (smart money flow, funding rate). Each pillar scores 0-100. Signal levels: None (<30), Weak (30-50), Moderate (50-70), Strong (70-85), VeryStrong (>85). Bottom scores indicate dip-buying opportunity; Top scores indicate selling/shorting opportunity. Prioritize the direction with highest total score.
Context may include `tbm_mtf` (Multi-Timeframe confirmation) — aggregated TBM scores across timeframes (15m, 1h, 4h, 1d, 1w). Key fields: bottom_score, top_score, bottom_alignment (how many TFs agree), has_conflict (opposing signals). Full alignment across 3+ TFs is a very strong signal. MTF conflict reduces confidence significantly. Higher timeframe signals (D1, W1) carry more weight than lower ones.
Context may include `trading_range` and `signal_dashboard` (primary engine row, slim summaries) plus `engine_timeframes`: same symbol across other `engine_symbols` intervals (each entry: interval, enabled, is_primary, slim `trading_range` + `signal_dashboard`). Prefer alignment across timeframes; when `engine_timeframes` disagree, weight higher intervals more unless `is_primary` interval shows a clear imminent setup.
{criteria}
Temperature: conservative; output JSON only."#
    )
}

fn operational_system_prompt_default(cfg: &AiEngineConfig) -> String {
    let locale = cfg.output_locale.as_deref().unwrap_or("en");
    format!(
        r#"You manage open positions (long AND short) for QTSS (operational layer). Reply JSON only, locale {locale}.
Required: "action" one of: keep, tighten_stop, widen_stop, activate_trailing, deactivate_trailing, partial_close, full_close, add_to_position.
For short positions: tighten_stop means moving stop closer to entry (lower), partial_close means buying back part of the short.
Optional: new_stop_loss_pct, new_take_profit_pct, trailing_callback_pct, partial_close_pct, reasoning.
Context may include `decision_history` — your recent decisions for this symbol. Maintain consistency; avoid contradicting recent actions without clear justification.
Context may include `ai_feedback` with past decision outcomes for this symbol: use win_rate and avg_pnl_pct to calibrate risk.
Never loosen stops without justification; prefer protecting capital."#
    )
}

fn strategic_system_prompt_default(cfg: &AiEngineConfig) -> String {
    let locale = cfg.output_locale.as_deref().unwrap_or("en");
    format!(
        r#"You are the strategic portfolio advisor for QTSS. Reply JSON only, locale {locale}.
Include when possible: "risk_budget_pct", "max_open_positions", "preferred_regime", "symbol_scores" (object symbol→weight 0-1), "macro_note".
No prose outside JSON."#
    )
}

/// `app_config` keys for prompt overrides.
const PROMPT_KEY_TACTICAL: &str = "ai_prompt_tactical";
const PROMPT_KEY_OPERATIONAL: &str = "ai_prompt_operational";
const PROMPT_KEY_STRATEGIC: &str = "ai_prompt_strategic";

/// Max concurrent tactical LLM calls per sweep.
const TACTICAL_MAX_CONCURRENCY: usize = 6;

/// One full tactical sweep over enabled engine symbols (FAZ 4.2 / 5).
/// Symbols are processed concurrently up to [`TACTICAL_MAX_CONCURRENCY`].
pub async fn run_tactical_sweep(rt: &AiRuntime) -> AiResult<()> {
    expire_stale_decisions(rt.pool()).await?;
    if !rt.config().enabled || !rt.config().tactical_layer_enabled {
        return Ok(());
    }
    let Some(provider) = rt.tactical_provider() else {
        if rt.config().tactical_layer_enabled {
            tracing::warn!(
                layer = "tactical",
                provider_config = %rt.config().provider_tactical,
                "tactical sweep skipped: provider not built (missing API key, wrong provider id, or layer misconfigured — no ai_decisions row is written)"
            );
        }
        return Ok(());
    };
    let symbols = list_enabled_engine_symbols(rt.pool()).await?;

    // Resolve prompt once (DB override or default).
    let cfg_clone = rt.config().clone();
    let system_prompt: Arc<str> = Arc::from(
        resolve_system_prompt(rt.pool(), PROMPT_KEY_TACTICAL, || tactical_system_prompt_default(&cfg_clone)).await,
    );

    // Shared references for spawned tasks.
    let pool = rt.pool().clone();
    let config = rt.config().clone();
    let provider = Arc::clone(provider);
    let breaker = Arc::clone(&rt.tactical_breaker);
    let notify = rt.notify_dispatcher().cloned();

    let semaphore = Arc::new(tokio::sync::Semaphore::new(TACTICAL_MAX_CONCURRENCY));
    let mut handles = Vec::with_capacity(symbols.len());

    for e in symbols {
        let sym = e.symbol.clone();
        let pool = pool.clone();
        let config = config.clone();
        let provider = Arc::clone(&provider);
        let breaker = Arc::clone(&breaker);
        let notify = notify.clone();
        let sem = Arc::clone(&semaphore);
        let prompt = Arc::clone(&system_prompt);

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await;
            if let Err(err) = run_tactical_single(
                &pool, &config, provider.as_ref(), &breaker, notify.as_ref(), &sym, &prompt,
            )
            .await
            {
                tracing::warn!(%sym, ?err, "tactical sweep symbol error");
            }
        });
        handles.push(handle);
    }

    for h in handles {
        let _ = h.await;
    }
    Ok(())
}

/// Process a single symbol in the tactical sweep.
async fn run_tactical_single(
    pool: &PgPool,
    config: &AiEngineConfig,
    provider: &dyn AiCompletionProvider,
    breaker: &CircuitBreaker,
    notify: Option<&NotificationDispatcher>,
    sym: &str,
    system_prompt: &str,
) -> AiResult<()> {
    let safety = SafetyConfig::from_ai_engine_config(config);
    let ctx = crate::context_builder::build_tactical_context(pool, sym).await?;
    let h = hash_context(&ctx);
    if decision_exists_for_hash(pool, &h, 30).await? {
        return Ok(());
    }
    let user = serde_json::to_string_pretty(&ctx).unwrap_or_else(|_| "{}".to_string());
    let is_gemini = provider.provider_id() == "gemini";
    let max_tokens = if is_gemini {
        config.max_tokens_tactical.max(4096)
    } else {
        config.max_tokens_tactical
    };
    let req = AiRequest {
        system: Some(system_prompt.to_string()),
        user,
        max_tokens,
        temperature: 0.3,
        model: config.model_tactical.clone(),
        force_json_mime: is_gemini,
    };
    let resp = match complete_with_retry(provider, &req, breaker).await {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!(?err, %sym, "tactical LLM call failed");
            let _ = insert_ai_decision_error(
                pool,
                "tactical",
                Some(sym),
                &ctx,
                &format!("provider_error: {err}"),
                &json!({ "provider": provider.provider_id() }),
            )
            .await;
            return Ok(());
        }
    };
    let mut parsed = match parse_tactical_decision(&resp.text) {
        Ok(p) => p,
        Err(err) => {
            let _ = insert_ai_decision_error(
                pool,
                "tactical",
                Some(sym),
                &ctx,
                &format!("parse_error: {err}; raw_head={}", &resp.text.chars().take(400).collect::<String>()),
                &json!({
                    "provider": resp.provider_id,
                    "model": resp.model,
                }),
            )
            .await;
            return Ok(());
        }
    };
    apply_strategic_portfolio_weight(&ctx, &mut parsed);
    let direction = parsed
        .get("direction")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if direction == "no_trade" {
        tracing::info!(%sym, "tactical LLM returned no_trade; skipping persistence");
        return Ok(());
    }
    let confidence = parsed.get("confidence").and_then(|x| x.as_f64()).unwrap_or(0.0);
    if confidence + f64::EPSILON < config.require_min_confidence {
        tracing::info!(
            %sym,
            confidence,
            min = config.require_min_confidence,
            "below minimum confidence; skipping persistence"
        );
        return Ok(());
    }
    if let Err(e) = validate_ai_decision_safety(&parsed, &safety) {
        tracing::warn!(%sym, err = e, "tactical decision failed safety");
        let _ = insert_ai_decision_error(
            pool,
            "tactical",
            Some(sym),
            &ctx,
            &format!("safety: {e}"),
            &json!({ "parsed": parsed }),
        )
        .await;
        return Ok(());
    }
    let valid_until = chrono::Utc::now()
        + chrono::Duration::seconds(config.decision_ttl_secs as i64);
    let meta = json!({
        "provider": resp.provider_id,
        "model": resp.model,
        "endpoint_host_hint": std::env::var("QTSS_AI_OPENAI_COMPAT_BASE_URL").ok().as_deref()
            .map(|s| s.split("//").nth(1).unwrap_or(s).split('/').next().unwrap_or("")),
        "usage": resp.usage,
    });
    let decision_id = insert_ai_decision(
        pool,
        "tactical",
        Some(sym),
        Some(&resp.model),
        Some(&h),
        &ctx,
        Some(&resp.text),
        Some(&parsed),
        Some(confidence),
        config.decision_ttl_secs,
        None,
        &meta,
    )
    .await?;
    insert_tactical_decision(pool, decision_id, sym, &parsed, valid_until).await?;
    let notify_snap = crate::approval::AiDecisionNotifySnapshot::from_tactical_context(&ctx, &parsed);
    maybe_auto_approve(
        pool,
        decision_id,
        confidence,
        config,
        notify,
        Some(sym),
        Some(direction),
        parsed.get("reasoning").and_then(|x| x.as_str()),
        &notify_snap,
    )
    .await?;
    Ok(())
}

/// Operational sweep: symbols with estimated net long (FAZ 6.1).
pub async fn run_operational_sweep(rt: &AiRuntime) -> AiResult<()> {
    expire_stale_decisions(rt.pool()).await?;
    if !rt.config().enabled || !rt.config().operational_layer_enabled {
        return Ok(());
    }
    let Some(provider) = rt.operational_provider() else {
        if rt.config().operational_layer_enabled {
            tracing::warn!(
                layer = "operational",
                provider_config = %rt.config().provider_operational,
                "operational sweep skipped: provider not built (missing API key or misconfiguration)"
            );
        }
        return Ok(());
    };
    let repo = ExchangeOrderRepository::new(rt.pool().clone());
    let rows = repo.list_recent_filled_orders_global(2000).await?;
    use rust_decimal::Decimal;
    let min_q = Decimal::new(1, 8);
    let symbols = symbols_with_open_positions_from_fills(&rows, min_q);
    let cfg_ref = rt.config().clone();
    let operational_prompt = resolve_system_prompt(
        rt.pool(), PROMPT_KEY_OPERATIONAL, || operational_system_prompt_default(&cfg_ref),
    ).await;
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
            system: Some(operational_prompt.clone()),
            user,
            max_tokens: rt.config().max_tokens_operational,
            temperature: 0.2,
            model: rt.config().model_operational.clone(),
            force_json_mime: false,
        };
        let resp = match complete_with_retry(provider.as_ref(), &req, rt.operational_breaker()).await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(?err, %sym, "operational LLM failed");
                let _ = insert_ai_decision_error(
                    rt.pool(),
                    "operational",
                    Some(&sym),
                    &ctx,
                    &format!("provider_error: {err}"),
                    &json!({ "provider": provider.provider_id() }),
                )
                .await;
                continue;
            }
        };
        let parsed = match parse_operational_decision(&resp.text) {
            Ok(p) => p,
            Err(err) => {
                tracing::warn!(%sym, ?err, "operational parse failed");
                let _ = insert_ai_decision_error(
                    rt.pool(),
                    "operational",
                    Some(&sym),
                    &ctx,
                    &format!("parse_error: {err}; raw_head={}", &resp.text.chars().take(400).collect::<String>()),
                    &json!({ "provider": resp.provider_id, "model": resp.model }),
                )
                .await;
                continue;
            }
        };
        let safety = SafetyConfig::from_ai_engine_config(rt.config());
        if let Err(e) = validate_operational_decision_safety(&parsed, &safety) {
            tracing::warn!(%sym, err = e, "operational decision failed safety");
            let _ = insert_ai_decision_error(
                rt.pool(),
                "operational",
                Some(&sym),
                &ctx,
                &format!("safety: {e}"),
                &json!({ "parsed": parsed }),
            )
            .await;
            continue;
        }
        let confidence = parsed.get("confidence").and_then(|x| x.as_f64()).unwrap_or(0.75);
        let meta = json!({ "provider": resp.provider_id, "model": resp.model, "usage": resp.usage });
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
        let notify_snap =
            crate::approval::AiDecisionNotifySnapshot::from_operational_context(&ctx, &parsed);
        maybe_auto_approve(
            rt.pool(),
            decision_id,
            confidence,
            rt.config(),
            rt.notify_dispatcher(),
            Some(&sym),
            parsed.get("action").and_then(|x| x.as_str()),
            parsed.get("reasoning").and_then(|x| x.as_str()),
            &notify_snap,
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
        if rt.config().strategic_layer_enabled {
            tracing::warn!(
                layer = "strategic",
                provider_config = %rt.config().provider_strategic,
                "strategic sweep skipped: provider not built (missing API key or misconfiguration)"
            );
        }
        return Ok(());
    };
    let ctx = crate::context_builder::build_strategic_context(rt.pool()).await?;
    let h = hash_context(&ctx);
    if decision_exists_for_hash(rt.pool(), &format!("st:{h}"), 60 * 24).await? {
        return Ok(());
    }
    let cfg_ref = rt.config().clone();
    let strategic_prompt = resolve_system_prompt(
        rt.pool(), PROMPT_KEY_STRATEGIC, || strategic_system_prompt_default(&cfg_ref),
    ).await;
    let user = serde_json::to_string_pretty(&ctx).unwrap_or_else(|_| "{}".to_string());
    let req = AiRequest {
        system: Some(strategic_prompt),
        user,
        max_tokens: rt.config().max_tokens_strategic,
        temperature: 0.35,
        model: rt.config().model_strategic.clone(),
        force_json_mime: false,
    };
    let resp = complete_with_retry(provider.as_ref(), &req, rt.strategic_breaker()).await?;
    let parsed = match parse_portfolio_decision(&resp.text) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(%e, "strategic portfolio JSON parse failed");
            let _ = insert_ai_decision_error(
                rt.pool(),
                "strategic",
                None,
                &ctx,
                &format!("parse_error: {e}; raw_head={}", &resp.text.chars().take(400).collect::<String>()),
                &json!({ "provider": resp.provider_id, "model": resp.model }),
            )
            .await;
            return Ok(());
        }
    };
    let safety = SafetyConfig::from_ai_engine_config(rt.config());
    if let Err(e) = validate_strategic_decision_safety(&parsed, &safety) {
        tracing::warn!(err = e, "strategic decision failed safety");
        let _ = insert_ai_decision_error(
            rt.pool(),
            "strategic",
            None,
            &ctx,
            &format!("safety: {e}"),
            &json!({ "parsed": parsed }),
        )
        .await;
        return Ok(());
    }
    let confidence = parsed.get("confidence").and_then(|x| x.as_f64()).unwrap_or(0.7);
    let meta = json!({ "provider": resp.provider_id, "model": resp.model, "usage": resp.usage });
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
    let notify_snap = crate::approval::AiDecisionNotifySnapshot::from_strategic_parsed(&parsed);
    maybe_auto_approve(
        rt.pool(),
        decision_id,
        confidence,
        rt.config(),
        rt.notify_dispatcher(),
        None,
        None,
        parsed.get("macro_note").and_then(|x| x.as_str()),
        &notify_snap,
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