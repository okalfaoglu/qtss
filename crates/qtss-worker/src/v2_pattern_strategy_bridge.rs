//! v2 pattern → strategy bridge — Faz 7 Adım 6.
//!
//! Subscribes to the in-process [`PATTERN_VALIDATED`] topic and hands
//! every incoming [`ValidatedDetection`] to a registered set of v2
//! `StrategyProvider`s. Emitted [`TradeIntent`]s are logged today; the
//! risk + execution wiring will pick them up off the bus in a later
//! step.
//!
//! Why a separate bridge module instead of inlining inside the
//! validator: CLAUDE.md #3 — detector / strategy / adapter separation.
//! The validator publishes; this bridge consumes. Either side can be
//! disabled or rewired without touching the other.
//!
//! Strategy registry is built as a `Vec<Box<dyn StrategyProvider>>`
//! (CLAUDE.md #1: trait dispatch, no scattered match arms). Adding a
//! new provider is one entry in [`build_providers`].

use std::sync::Arc;

use qtss_domain::execution::ExecutionMode;
use qtss_domain::v2::detection::ValidatedDetection;
use qtss_eventbus::{
    topics::{INTENT_CREATED, PATTERN_VALIDATED},
    EventBus, EventBusError, InProcessBus,
};
use qtss_storage::{
    resolve_system_f64, resolve_system_string, resolve_system_u64, resolve_worker_enabled_flag,
};
use qtss_strategy::v2::{
    ConfidenceThresholdStrategy, ConfidenceThresholdStrategyConfig, StrategyContext,
    StrategyProvider,
};
use qtss_domain::v2::intent::TimeInForce;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromStr;
use sqlx::PgPool;
use tracing::{debug, info, warn};

pub async fn v2_pattern_strategy_bridge_loop(pool: PgPool, bus: Arc<InProcessBus>) {
    info!("v2 pattern→strategy bridge spawned (subscribes to {PATTERN_VALIDATED})");

    let providers = build_providers(&pool).await;
    if providers.is_empty() {
        warn!("v2 pattern→strategy bridge: no providers registered, exiting");
        return;
    }

    let ctx = StrategyContext {
        run_mode: resolve_run_mode(&pool).await,
    };

    let mut stream = bus.subscribe::<ValidatedDetection>(PATTERN_VALIDATED);
    loop {
        match stream.recv().await {
            Ok(Some(event)) => dispatch(&providers, &ctx, &event.payload, &bus).await,
            Ok(None) => {
                // Foreign payload on the topic — already logged by the bus.
            }
            Err(EventBusError::Lagged { topic, skipped }) => {
                warn!(%topic, skipped, "v2 strategy bridge lagged, resuming");
            }
            Err(e) => {
                warn!(%e, "v2 strategy bridge stream closed");
                return;
            }
        }
    }
}

async fn dispatch(
    providers: &[Box<dyn StrategyProvider>],
    ctx: &StrategyContext,
    signal: &ValidatedDetection,
    bus: &InProcessBus,
) {
    for provider in providers {
        match provider.evaluate(signal, ctx) {
            Ok(intents) if intents.is_empty() => {
                debug!(strategy = provider.id(), "pass (no intents)");
            }
            Ok(intents) => {
                for intent in intents {
                    info!(
                        strategy = provider.id(),
                        symbol = %intent.instrument.symbol,
                        side = ?intent.side,
                        conviction = intent.conviction,
                        "v2 strategy bridge: trade intent emitted"
                    );
                    if let Err(e) = bus.publish(INTENT_CREATED, &intent).await {
                        warn!(%e, "failed to publish intent.created");
                    }
                }
            }
            Err(e) => warn!(strategy = provider.id(), %e, "strategy evaluate failed"),
        }
    }
}

/// Strategy registry — Faz 7 Adım 11. Each provider entry is gated by
/// its own `strategy.<id>.enabled` flag in `system_config` and reads
/// its tunables from the same module so the operator can rewire the
/// bridge without a deploy (CLAUDE.md #2). Adding a provider is one
/// builder call here plus a config seed migration — no central match
/// arm to edit (CLAUDE.md #1).
async fn build_providers(pool: &PgPool) -> Vec<Box<dyn StrategyProvider>> {
    let mut providers: Vec<Box<dyn StrategyProvider>> = Vec::new();

    if resolve_worker_enabled_flag(
        pool,
        "strategy",
        "confidence_threshold.enabled",
        "QTSS_STRATEGY_CONFIDENCE_THRESHOLD_ENABLED",
        true,
    )
    .await
    {
        if let Some(p) = build_confidence_threshold(pool).await {
            providers.push(p);
        }
    }

    providers
}

async fn build_confidence_threshold(pool: &PgPool) -> Option<Box<dyn StrategyProvider>> {
    let min_confidence = resolve_system_f64(
        pool,
        "strategy",
        "confidence_threshold.min_confidence",
        "QTSS_STRATEGY_CONFIDENCE_THRESHOLD_MIN_CONFIDENCE",
        0.6,
    )
    .await as f32;
    let risk_pct_f = resolve_system_f64(
        pool,
        "strategy",
        "confidence_threshold.risk_pct",
        "QTSS_STRATEGY_CONFIDENCE_THRESHOLD_RISK_PCT",
        0.005,
    )
    .await;
    let risk_pct = Decimal::from_str(&format!("{risk_pct_f}")).unwrap_or(Decimal::ZERO);
    let tif_raw = resolve_system_string(
        pool,
        "strategy",
        "confidence_threshold.time_in_force",
        "QTSS_STRATEGY_CONFIDENCE_THRESHOLD_TIF",
        "gtc",
    )
    .await;
    let time_in_force = parse_tif(&tif_raw);
    let time_stop_secs_raw = resolve_system_u64(
        pool,
        "strategy",
        "confidence_threshold.time_stop_secs",
        "QTSS_STRATEGY_CONFIDENCE_THRESHOLD_TIME_STOP_SECS",
        0,
        0,
        7 * 24 * 3600,
    )
    .await;
    let time_stop_secs = (time_stop_secs_raw > 0).then_some(time_stop_secs_raw as i64);
    let act_on_forming = resolve_worker_enabled_flag(
        pool,
        "strategy",
        "confidence_threshold.act_on_forming",
        "QTSS_STRATEGY_CONFIDENCE_THRESHOLD_ACT_ON_FORMING",
        false,
    )
    .await;

    let cfg = ConfidenceThresholdStrategyConfig {
        min_confidence,
        risk_pct,
        time_in_force,
        time_stop_secs,
        act_on_forming,
    };
    match ConfidenceThresholdStrategy::new("v2.confidence_threshold", cfg) {
        Ok(s) => Some(Box::new(s)),
        Err(e) => {
            warn!(%e, "ConfidenceThresholdStrategy init failed");
            None
        }
    }
}

fn parse_tif(s: &str) -> TimeInForce {
    match s.trim().to_lowercase().as_str() {
        "ioc" => TimeInForce::Ioc,
        "fok" => TimeInForce::Fok,
        "day" => TimeInForce::Day,
        _ => TimeInForce::Gtc,
    }
}

async fn resolve_run_mode(pool: &PgPool) -> ExecutionMode {
    let raw = resolve_system_string(
        pool,
        "worker",
        "runtime_mode",
        "QTSS_RUNTIME_MODE",
        "dry",
    )
    .await;
    match raw.trim().to_lowercase().as_str() {
        "live" => ExecutionMode::Live,
        "backtest" => ExecutionMode::Backtest,
        _ => ExecutionMode::Dry,
    }
}
