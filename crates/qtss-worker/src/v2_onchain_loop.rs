//! Faz 7.7 / B4 — v2 onchain fetcher loop.
//!
//! For every enabled engine_symbol, dispatches the registered
//! [`OnchainCategoryFetcher`]s in parallel, blends them with
//! [`qtss_onchain::aggregate`], and writes one row into
//! `qtss_v2_onchain_metrics`. The TBM detector reads the latest fresh
//! row through [`crate::v2_onchain_bridge::StoredV2OnchainProvider`].
//!
//! All thresholds, weights and enable flags are config-driven
//! (CLAUDE.md #2). The fetcher list itself is built once per pass via
//! a single [`build_fetchers`] dispatch table — adding a new fetcher
//! is one row, no scattered branching (CLAUDE.md #1).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use qtss_onchain::{
    aggregate, AggregatorWeights, BinanceDerivativesFetcher, CategoryKind, CategoryReading,
    CryptoQuantFetcher, GlassnodeFetcher, NansenFetcher, NansenTuning, OnchainCategoryFetcher,
    StablecoinMacroFetcher,
};
use qtss_storage::{
    list_enabled_engine_symbols, resolve_system_f64, resolve_system_string, resolve_system_u64,
    resolve_worker_enabled_flag,
    SystemConfigRepository,
    v2_onchain_metrics::{insert_v2_onchain_metrics, V2OnchainMetricsInsert},
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
struct LoopConfig {
    enabled: bool,
    tick_interval_s: u64,
    weights: AggregatorWeights,
    derivatives_on: bool,
    stablecoin_on: bool,
    glassnode_on: bool,
    glassnode_key: String,
    cryptoquant_on: bool,
    cryptoquant_key: String,
    nansen_on: bool,
    nansen_tuning: NansenTuning,
}

pub async fn v2_onchain_loop(pool: PgPool) {
    info!("v2 onchain loop spawned (gated on onchain.enabled)");
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    loop {
        let cfg = load_config(&pool).await;
        if !cfg.enabled {
            tokio::time::sleep(Duration::from_secs(cfg.tick_interval_s)).await;
            continue;
        }

        match run_pass(&pool, &http, &cfg).await {
            Ok(n) if n > 0 => info!(symbols = n, "v2 onchain pass complete"),
            Ok(_) => debug!("v2 onchain pass: no enabled symbols"),
            Err(e) => warn!(%e, "v2 onchain pass failed"),
        }

        tokio::time::sleep(Duration::from_secs(cfg.tick_interval_s)).await;
    }
}

async fn load_config(pool: &PgPool) -> LoopConfig {
    let enabled = resolve_worker_enabled_flag(
        pool, "onchain", "enabled", "QTSS_ONCHAIN_V2_ENABLED", false,
    )
    .await;
    let tick_interval_s = resolve_system_u64(
        pool, "onchain", "tick_interval_s", "QTSS_ONCHAIN_V2_TICK", 300, 30, 86_400,
    )
    .await;
    let weights = AggregatorWeights {
        derivatives: resolve_system_f64(
            pool, "onchain", "aggregator.weight.derivatives", "QTSS_ONCHAIN_W_DERIV", 0.5,
        )
        .await,
        stablecoin: resolve_system_f64(
            pool, "onchain", "aggregator.weight.stablecoin", "QTSS_ONCHAIN_W_STABLE", 0.3,
        )
        .await,
        chain: resolve_system_f64(
            pool, "onchain", "aggregator.weight.chain", "QTSS_ONCHAIN_W_CHAIN", 0.2,
        )
        .await,
    };
    let derivatives_on = resolve_worker_enabled_flag(
        pool, "onchain", "fetcher.derivatives.enabled", "QTSS_ONCHAIN_DERIV_ON", true,
    )
    .await;
    let stablecoin_on = resolve_worker_enabled_flag(
        pool, "onchain", "fetcher.stablecoin.enabled", "QTSS_ONCHAIN_STABLE_ON", true,
    )
    .await;
    let glassnode_on = resolve_worker_enabled_flag(
        pool, "onchain", "fetcher.glassnode.enabled", "QTSS_ONCHAIN_GN_ON", false,
    )
    .await;
    let glassnode_key = resolve_system_string(
        pool, "onchain", "fetcher.glassnode.api_key", "QTSS_GLASSNODE_API_KEY", "",
    )
    .await;
    let cryptoquant_on = resolve_worker_enabled_flag(
        pool, "onchain", "fetcher.cryptoquant.enabled", "QTSS_ONCHAIN_CQ_ON", false,
    )
    .await;
    let cryptoquant_key = resolve_system_string(
        pool, "onchain", "fetcher.cryptoquant.api_key", "QTSS_CRYPTOQUANT_API_KEY", "",
    )
    .await;
    let nansen_on = resolve_worker_enabled_flag(
        pool, "onchain", "fetcher.nansen.enabled", "QTSS_ONCHAIN_NANSEN_ON", false,
    )
    .await;
    let nansen_tuning = NansenTuning {
        staleness_s: resolve_system_u64(
            pool, "onchain", "fetcher.nansen.staleness_s", "QTSS_ONCHAIN_NANSEN_STALE",
            7200, 60, 86_400,
        )
        .await as i64,
        w_netflow: resolve_system_f64(
            pool, "onchain", "fetcher.nansen.weight.netflow", "QTSS_ONCHAIN_NANSEN_W_NF", 0.40,
        )
        .await,
        w_flow_intel: resolve_system_f64(
            pool, "onchain", "fetcher.nansen.weight.flow_intel", "QTSS_ONCHAIN_NANSEN_W_FI", 0.25,
        )
        .await,
        w_dex_trades: resolve_system_f64(
            pool, "onchain", "fetcher.nansen.weight.dex_trades", "QTSS_ONCHAIN_NANSEN_W_DX", 0.20,
        )
        .await,
        w_holdings: resolve_system_f64(
            pool, "onchain", "fetcher.nansen.weight.holdings", "QTSS_ONCHAIN_NANSEN_W_HD", 0.15,
        )
        .await,
        symbol_map: load_nansen_symbol_map(pool).await,
    };

    LoopConfig {
        enabled,
        tick_interval_s,
        weights,
        derivatives_on,
        stablecoin_on,
        glassnode_on,
        glassnode_key,
        cryptoquant_on,
        cryptoquant_key,
        nansen_on,
        nansen_tuning,
    }
}

/// Reads `onchain.nansen.symbol_map` as raw JSON object. Returns
/// `Value::Object({})` when the row is missing or not an object so the
/// fetcher just emits `UnsupportedSymbol` for everything.
async fn load_nansen_symbol_map(pool: &PgPool) -> Value {
    let repo = SystemConfigRepository::new(pool.clone());
    match repo.get("onchain", "nansen.symbol_map").await {
        Ok(Some(row)) if row.value.is_object() => row.value,
        _ => Value::Object(Default::default()),
    }
}

/// Single dispatch table — order is irrelevant since the aggregator
/// keys off `category()`. Adding a new fetcher is one push().
fn build_fetchers(
    pool: &PgPool,
    http: &reqwest::Client,
    cfg: &LoopConfig,
) -> Vec<Arc<dyn OnchainCategoryFetcher>> {
    let mut out: Vec<Arc<dyn OnchainCategoryFetcher>> = Vec::new();
    if cfg.derivatives_on {
        out.push(Arc::new(BinanceDerivativesFetcher::new(
            http.clone(),
            Default::default(),
        )));
    }
    if cfg.stablecoin_on {
        out.push(Arc::new(StablecoinMacroFetcher::new(
            http.clone(),
            Default::default(),
        )));
    }
    if cfg.glassnode_on {
        if let Some(gn) = GlassnodeFetcher::new(
            http.clone(),
            Some(cfg.glassnode_key.clone()),
            Default::default(),
        ) {
            out.push(Arc::new(gn));
        }
    }
    if cfg.cryptoquant_on {
        if let Some(cq) = CryptoQuantFetcher::new(
            http.clone(),
            Some(cfg.cryptoquant_key.clone()),
            Default::default(),
        ) {
            out.push(Arc::new(cq));
        }
    }
    if cfg.nansen_on {
        out.push(Arc::new(NansenFetcher::new(
            pool.clone(),
            cfg.nansen_tuning.clone(),
        )));
    }
    out
}

async fn run_pass(
    pool: &PgPool,
    http: &reqwest::Client,
    cfg: &LoopConfig,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let fetchers = build_fetchers(pool, http, cfg);
    if fetchers.is_empty() {
        return Ok(0);
    }

    let symbols = list_enabled_engine_symbols(pool).await?;
    // Dedupe by symbol — multiple timeframes share the same on-chain row.
    let mut seen: HashSet<String> = HashSet::new();
    let mut processed = 0usize;
    for s in symbols {
        let sym = s.symbol.trim().to_uppercase();
        if !seen.insert(sym.clone()) {
            continue;
        }
        match process_symbol(pool, &fetchers, &sym, cfg).await {
            Ok(()) => processed += 1,
            Err(e) => warn!(symbol = %sym, %e, "onchain fetch failed"),
        }
    }
    Ok(processed)
}

async fn process_symbol(
    pool: &PgPool,
    fetchers: &[Arc<dyn OnchainCategoryFetcher>],
    symbol: &str,
    cfg: &LoopConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Fan out fetchers in parallel; each contributes one CategoryReading.
    let mut handles = Vec::with_capacity(fetchers.len());
    for f in fetchers {
        let f = f.clone();
        let sym = symbol.to_string();
        handles.push(tokio::spawn(async move { f.fetch(&sym).await }));
    }

    let mut readings: Vec<CategoryReading> = Vec::new();
    for h in handles {
        match h.await {
            Ok(Ok(r)) => readings.push(r),
            Ok(Err(e)) => debug!(symbol = %symbol, %e, "category fetch error (skipping)"),
            Err(e) => debug!(symbol = %symbol, %e, "join error"),
        }
    }

    let agg = aggregate(&readings, cfg.weights);

    let pick = |k: CategoryKind| -> Option<f64> {
        readings.iter().find(|r| r.category == k).map(|r| r.score)
    };

    let raw_meta = json!({
        "details": agg.details,
        "per_category": agg.per_category,
        "fetcher_count": readings.len(),
    });

    insert_v2_onchain_metrics(
        pool,
        &V2OnchainMetricsInsert {
            symbol: symbol.to_string(),
            derivatives_score: pick(CategoryKind::Derivatives),
            stablecoin_score: pick(CategoryKind::Stablecoin),
            chain_score: pick(CategoryKind::Chain),
            aggregate_score: agg.score,
            direction: agg.direction.as_str().to_string(),
            confidence: agg.confidence,
            raw_meta,
        },
    )
    .await?;
    Ok(())
}
