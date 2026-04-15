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
    nansen_enriched::{self, EnrichedConfig, EnrichedSignal},
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
    enriched: EnrichedConfig,
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

    let enriched = EnrichedConfig {
        enabled: resolve_worker_enabled_flag(
            pool, "onchain", "nansen.enriched.enabled", "QTSS_NANSEN_ENRICHED", false,
        ).await,
        cross_chain_min_chains: resolve_system_u64(
            pool, "onchain", "nansen.enriched.cross_chain.min_chains", "QTSS_NANSEN_CC_MIN", 2, 2, 10,
        ).await as usize,
        cross_chain_agreement_boost: resolve_system_f64(
            pool, "onchain", "nansen.enriched.cross_chain.agreement_boost", "QTSS_NANSEN_CC_BOOST", 0.3,
        ).await,
        dex_spike_threshold_x: resolve_system_f64(
            pool, "onchain", "nansen.enriched.dex_spike.threshold_x", "QTSS_NANSEN_SPIKE_X", 3.0,
        ).await,
        dex_spike_min_value_usd: resolve_system_f64(
            pool, "onchain", "nansen.enriched.dex_spike.min_value_usd", "QTSS_NANSEN_SPIKE_MIN", 50_000.0,
        ).await,
        whale_top_n: resolve_system_u64(
            pool, "onchain", "nansen.enriched.whale.top_n", "QTSS_NANSEN_WHALE_N", 10, 1, 100,
        ).await as usize,
        whale_delta_threshold: resolve_system_f64(
            pool, "onchain", "nansen.enriched.whale.delta_threshold", "QTSS_NANSEN_WHALE_D", 0.05,
        ).await,
        w_cross_chain: resolve_system_f64(
            pool, "onchain", "nansen.enriched.weight.cross_chain", "QTSS_NANSEN_W_CC", 0.15,
        ).await,
        w_dex_spike: resolve_system_f64(
            pool, "onchain", "nansen.enriched.weight.dex_spike", "QTSS_NANSEN_W_SPIKE", 0.10,
        ).await,
        w_whale_conc: resolve_system_f64(
            pool, "onchain", "nansen.enriched.weight.whale_conc", "QTSS_NANSEN_W_WHALE", 0.10,
        ).await,
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
        enriched,
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
        if !qtss_storage::is_backfill_ready(pool, s.id).await {
            continue;
        }
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

    let has_chain = readings.iter().any(|r| r.category == CategoryKind::Chain);
    if !has_chain {
        debug!(symbol = %symbol, "no chain reading — check NANSEN_API_KEY / CryptoQuant key validity");
    }

    // ── Enriched analysis (cross-chain, DEX spike, whale) ──────────
    #[allow(unused_assignments)]
    let mut enriched_signals: Vec<EnrichedSignal> = Vec::new();
    let mut enriched_meta = json!({});
    if cfg.enriched.enabled {
        debug!(symbol = %symbol, "enriched analysis running");
        if let Some(enriched_result) = run_enriched_analysis(pool, symbol, &cfg.nansen_tuning, &cfg.enriched).await {
            enriched_signals = enriched_result.signals;
            enriched_meta = enriched_result.meta;
            // If enriched produced a blended score, inject as chain reading
            if let Some((escore, econf)) = nansen_enriched::blend_enriched(&enriched_signals, &cfg.enriched) {
                if econf > 0.0 {
                    readings.push(CategoryReading {
                        category: CategoryKind::Chain,
                        score: escore,
                        confidence: econf,
                        direction: Some(if escore > 0.05 {
                            qtss_onchain::OnchainDirection::Long
                        } else if escore < -0.05 {
                            qtss_onchain::OnchainDirection::Short
                        } else {
                            qtss_onchain::OnchainDirection::Neutral
                        }),
                        details: vec![format!("[enriched] score={escore:.2} conf={econf:.2}")],
                    });
                }
            }
        }
    }

    // Re-aggregate with enriched readings included
    let agg = aggregate(&readings, cfg.weights);
    let pick = |k: CategoryKind| -> Option<f64> {
        readings.iter().find(|r| r.category == k).map(|r| r.score)
    };

    let raw_meta = json!({
        "details": agg.details,
        "per_category": agg.per_category,
        "fetcher_count": readings.len(),
        "enriched": enriched_meta,
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

// ── Enriched analysis runner ───────────────────────────────────────

struct EnrichedResult {
    signals: Vec<EnrichedSignal>,
    meta: Value,
}

async fn run_enriched_analysis(
    pool: &PgPool,
    symbol: &str,
    nansen_tuning: &NansenTuning,
    ecfg: &EnrichedConfig,
) -> Option<EnrichedResult> {
    let key = nansen_enriched::parse_multi_chain_keys(&nansen_tuning.symbol_map, symbol)?;

    // Load the same snapshots the NansenFetcher uses
    let staleness = nansen_tuning.staleness_s;
    let netflow_resp = load_snapshot(pool, "nansen_netflows", staleness).await;
    let dex_resp = load_snapshot(pool, "nansen_smart_money_dex_trades", staleness).await;
    let holdings_resp = load_snapshot(pool, "nansen_holdings", staleness).await;

    debug!(
        symbol = %symbol,
        netflow = netflow_resp.is_some(),
        dex = dex_resp.is_some(),
        holdings = holdings_resp.is_some(),
        chains = key.chains.len(),
        "enriched: snapshots loaded"
    );

    if netflow_resp.is_none() && dex_resp.is_none() && holdings_resp.is_none() {
        return None;
    }

    let mut signals: Vec<EnrichedSignal> = Vec::new();

    // 1. Cross-chain flow
    let cc_sig = nansen_enriched::analyze_cross_chain_flow(netflow_resp.as_ref(), &key, ecfg);
    debug!(symbol = %symbol, has_signal = cc_sig.is_some(), "enriched: cross_chain_flow");
    if let Some(sig) = cc_sig {
        persist_enriched(pool, symbol, &sig).await;
        signals.push(sig);
    }

    // 2. DEX volume spike (get previous baseline)
    let prev_vol = qtss_storage::nansen_enriched::fetch_latest_enriched(
        pool, symbol, "dex_volume_spike", staleness,
    )
    .await
    .ok()
    .flatten()
    .and_then(|r| r.details.as_ref().and_then(|d| d.get("total_volume_usd").and_then(|v| v.as_f64())));

    let dex_sig = nansen_enriched::analyze_dex_volume_spike(dex_resp.as_ref(), &key, ecfg, prev_vol);
    debug!(symbol = %symbol, has_signal = dex_sig.is_some(), prev_vol = ?prev_vol, "enriched: dex_volume_spike");
    if let Some(sig) = dex_sig {
        if sig.details.as_ref().and_then(|d| d.get("is_spike")).and_then(|v| v.as_bool()).unwrap_or(false) {
            fire_enriched_alert(pool, symbol, &sig, ecfg).await;
        }
        persist_enriched(pool, symbol, &sig).await;
        signals.push(sig);
    }

    // 3. Whale concentration
    let prev_conc = qtss_storage::nansen_enriched::fetch_latest_enriched(
        pool, symbol, "whale_concentration", staleness,
    )
    .await
    .ok()
    .flatten()
    .and_then(|r| r.details.as_ref().and_then(|d| d.get("top_n_avg_change_pct").and_then(|v| v.as_f64())));

    let whale_sig = nansen_enriched::analyze_whale_concentration(holdings_resp.as_ref(), &key, ecfg, prev_conc);
    debug!(symbol = %symbol, has_signal = whale_sig.is_some(), prev_conc = ?prev_conc, "enriched: whale_concentration");
    if let Some(sig) = whale_sig {
        if sig.confidence > 0.6 {
            fire_enriched_alert(pool, symbol, &sig, ecfg).await;
        }
        persist_enriched(pool, symbol, &sig).await;
        signals.push(sig);
    }

    let sig_count = signals.len();
    let meta = json!({
        "enriched_signals": sig_count,
        "types": signals.iter().map(|s| s.signal_type).collect::<Vec<_>>(),
    });

    if sig_count > 0 {
        debug!(symbol = %symbol, count = sig_count, "enriched signals produced");
    }

    Some(EnrichedResult { signals, meta })
}

async fn load_snapshot(pool: &PgPool, key: &str, staleness_s: i64) -> Option<Value> {
    let row = qtss_storage::data_snapshots::fetch_data_snapshot(pool, key)
        .await
        .ok()
        .flatten()?;
    if row.error.is_some() {
        return None;
    }
    let age = chrono::Utc::now()
        .signed_duration_since(row.computed_at)
        .num_seconds();
    if age > staleness_s {
        return None;
    }
    row.response_json
}

async fn persist_enriched(pool: &PgPool, symbol: &str, sig: &EnrichedSignal) {
    let insert = qtss_storage::nansen_enriched::EnrichedSignalInsert {
        symbol,
        signal_type: sig.signal_type,
        score: sig.score,
        direction: sig.direction,
        confidence: sig.confidence,
        chain_breakdown: sig.chain_breakdown.clone(),
        details: sig.details.clone(),
    };
    if let Err(e) = qtss_storage::nansen_enriched::insert_enriched_signal(pool, &insert).await {
        warn!(%e, symbol, signal_type = sig.signal_type, "enriched signal insert failed");
    }
}

async fn fire_enriched_alert(
    pool: &PgPool,
    symbol: &str,
    sig: &EnrichedSignal,
    _ecfg: &EnrichedConfig,
) {
    let repo = qtss_storage::NotifyOutboxRepository::new(pool.clone());
    let event_key = format!("nansen_{}", sig.signal_type);

    // Dedupe: 1 hour per (event_key, symbol)
    match repo.exists_recent_global_event_symbol(&event_key, symbol, 3600).await {
        Ok(true) => return,
        Ok(false) => {}
        Err(e) => {
            warn!(%e, "enriched alert dedupe check failed");
            return;
        }
    }

    let title = match sig.signal_type {
        "dex_volume_spike" => format!("{symbol}: SM DEX Volume Spike"),
        "whale_concentration" => format!("{symbol}: Whale Konsantrasyon Degisimi"),
        "cross_chain_flow" => format!("{symbol}: Cross-Chain Akis Sinyali"),
        _ => format!("{symbol}: Nansen Sinyal"),
    };
    let body = format!(
        "Skor: {:.2} | Yon: {} | Guven: {:.0}%\n{}",
        sig.score,
        sig.direction,
        sig.confidence * 100.0,
        sig.details
            .as_ref()
            .map(|d| serde_json::to_string_pretty(d).unwrap_or_default())
            .unwrap_or_default()
    );

    if let Err(e) = repo
        .enqueue_with_meta(
            None,
            Some(&event_key),
            "warning",
            None,
            None,
            Some(symbol),
            &title,
            &body,
            vec!["telegram".to_string()],
        )
        .await
    {
        warn!(%e, "enriched alert enqueue failed");
    } else {
        info!(symbol, signal_type = sig.signal_type, "enriched alert enqueued");
    }
}
