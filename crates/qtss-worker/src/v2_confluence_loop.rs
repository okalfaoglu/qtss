//! Faz 7.8 — v2 confluence scoring loop.
//!
//! For every enabled `(exchange, symbol, timeframe)` row this loop:
//!
//! 1. Reads recent `qtss_v2_detections` rows inside a configurable
//!    freshness window (default 300s).
//! 2. Splits them into the TBM aggregate vote and per-family
//!    structural votes that the [`qtss_confluence`] scorer expects.
//! 3. Loads the latest fresh `qtss_v2_onchain_metrics` row for the
//!    same symbol via the existing TBM bridge helper.
//! 4. Calls [`qtss_confluence::score_confluence`] and writes one row
//!    into `qtss_v2_confluence`.
//!
//! All weights and the freshness window live in `system_config`
//! (CLAUDE.md #2). Direction inference for a detection row uses a
//! single keyword dispatch table over `subkind` — no scattered if/else
//! (CLAUDE.md #1). Adding a new family is a one-line entry.

use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use qtss_confluence::{
    score_confluence, ConfluenceDirection, ConfluenceInputs, ConfluenceReading, ConfluenceWeights,
    DetectionVote,
};
use qtss_storage::v2_confluence::{insert_v2_confluence, V2ConfluenceInsert};
use qtss_storage::v2_onchain_metrics::fetch_latest_for_tbm;
use qtss_storage::{
    list_enabled_engine_symbols, resolve_system_f64, resolve_system_u64,
    resolve_worker_enabled_flag, DetectionFilter, DetectionRow, EngineSymbolRow,
    V2DetectionRepository,
};
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
struct LoopConfig {
    enabled: bool,
    tick_interval_s: u64,
    window_s: i64,
    weights: ConfluenceWeights,
}

pub async fn v2_confluence_loop(pool: PgPool) {
    info!("v2 confluence loop spawned (gated on confluence.enabled)");

    loop {
        let cfg = load_config(&pool).await;
        if !cfg.enabled {
            tokio::time::sleep(Duration::from_secs(cfg.tick_interval_s)).await;
            continue;
        }

        match run_pass(&pool, &cfg).await {
            Ok(n) if n > 0 => info!(rows = n, "v2 confluence pass complete"),
            Ok(_) => debug!("v2 confluence pass: no fresh inputs"),
            Err(e) => warn!(%e, "v2 confluence pass failed"),
        }

        tokio::time::sleep(Duration::from_secs(cfg.tick_interval_s)).await;
    }
}

async fn load_config(pool: &PgPool) -> LoopConfig {
    let enabled = resolve_worker_enabled_flag(
        pool, "confluence", "enabled", "QTSS_CONFLUENCE_ENABLED", false,
    )
    .await;
    let tick_interval_s = resolve_system_u64(
        pool, "confluence", "tick_interval_s", "QTSS_CONFLUENCE_TICK", 30, 5, 3600,
    )
    .await;
    let window_s = resolve_system_u64(
        pool, "confluence", "window_s", "QTSS_CONFLUENCE_WINDOW", 300, 30, 86_400,
    )
    .await as i64;

    let weights = ConfluenceWeights {
        elliott: resolve_system_f64(
            pool, "confluence", "weight.elliott", "QTSS_CONFLUENCE_W_ELLIOTT", 0.30,
        )
        .await,
        harmonic: resolve_system_f64(
            pool, "confluence", "weight.harmonic", "QTSS_CONFLUENCE_W_HARMONIC", 0.20,
        )
        .await,
        classical: resolve_system_f64(
            pool, "confluence", "weight.classical", "QTSS_CONFLUENCE_W_CLASSICAL", 0.15,
        )
        .await,
        wyckoff: resolve_system_f64(
            pool, "confluence", "weight.wyckoff", "QTSS_CONFLUENCE_W_WYCKOFF", 0.15,
        )
        .await,
        range: resolve_system_f64(
            pool, "confluence", "weight.range", "QTSS_CONFLUENCE_W_RANGE", 0.10,
        )
        .await,
        tbm: resolve_system_f64(
            pool, "confluence", "weight.tbm", "QTSS_CONFLUENCE_W_TBM", 0.10,
        )
        .await,
        onchain: resolve_system_f64(
            pool, "confluence", "weight.onchain", "QTSS_CONFLUENCE_W_ONCHAIN", 0.10,
        )
        .await,
        min_layers: resolve_system_u64(
            pool, "confluence", "min_layers", "QTSS_CONFLUENCE_MIN_LAYERS", 3, 1, 20,
        )
        .await as u32,
    };

    LoopConfig {
        enabled,
        tick_interval_s,
        window_s,
        weights,
    }
}

/// Map a detection `subkind` string to a direction. Single dispatch
/// table — adding a new keyword pair is one row, no scattered if/else.
fn direction_from_subkind(subkind: &str) -> ConfluenceDirection {
    let s = subkind.to_ascii_lowercase();
    const LONG_KEYS: &[&str] = &[
        "bottom", "long", "buy", "bull", "accumulation", "spring", "support",
    ];
    const SHORT_KEYS: &[&str] = &[
        "top", "short", "sell", "bear", "distribution", "upthrust", "resistance",
    ];
    if LONG_KEYS.iter().any(|k| s.contains(k)) {
        return ConfluenceDirection::Long;
    }
    if SHORT_KEYS.iter().any(|k| s.contains(k)) {
        return ConfluenceDirection::Short;
    }
    ConfluenceDirection::Neutral
}

/// TBM rows live in `qtss_v2_detections` with `family="tbm"`. Map
/// `subkind` + `structural_score` into the `[-1, +1]` aggregate the
/// confluence scorer expects.
fn tbm_score_from_row(row: &DetectionRow) -> f64 {
    let dir = direction_from_subkind(&row.subkind);
    dir.sign() * row.structural_score as f64
}

async fn run_pass(
    pool: &PgPool,
    cfg: &LoopConfig,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let symbols = list_enabled_engine_symbols(pool).await?;
    let repo = V2DetectionRepository::new(pool.clone());
    let cutoff = Utc::now() - ChronoDuration::seconds(cfg.window_s);

    let mut written = 0usize;
    for sym in symbols {
        if !qtss_storage::is_backfill_ready(pool, sym.id).await {
            continue;
        }
        match process_symbol(pool, &repo, &sym, cfg, cutoff).await {
            Ok(true) => written += 1,
            Ok(false) => {}
            Err(e) => warn!(symbol = %sym.symbol, interval = %sym.interval, %e, "confluence symbol failed"),
        }
    }
    Ok(written)
}

async fn process_symbol(
    pool: &PgPool,
    repo: &V2DetectionRepository,
    sym: &EngineSymbolRow,
    cfg: &LoopConfig,
    cutoff: chrono::DateTime<Utc>,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let rows = repo
        .list_filtered(DetectionFilter {
            exchange: Some(&sym.exchange),
            symbol: Some(&sym.symbol),
            timeframe: Some(&sym.interval),
            family: None,
            state: None,
            mode: None,
            limit: 200,
        })
        .await?;

    // Split fresh rows into TBM (aggregate) and structural (votes).
    let mut tbm_score: Option<f64> = None;
    let mut tbm_confidence: Option<f64> = None;
    let mut detections: Vec<DetectionVote> = Vec::new();

    for row in rows.iter().filter(|r| r.detected_at >= cutoff) {
        if row.family.eq_ignore_ascii_case("tbm") {
            // Most recent TBM row wins (rows arrive DESC).
            if tbm_score.is_none() {
                tbm_score = Some(tbm_score_from_row(row));
                tbm_confidence = row.confidence.map(|c| c as f64);
            }
            continue;
        }
        detections.push(DetectionVote {
            family: row.family.clone(),
            subkind: row.subkind.clone(),
            direction: direction_from_subkind(&row.subkind),
            structural_score: row.structural_score,
        });
    }

    // Onchain aggregate — fresh window matches confluence window.
    let onchain = fetch_latest_for_tbm(
        pool,
        &sym.symbol,
        ChronoDuration::seconds(cfg.window_s),
    )
    .await
    .ok()
    .flatten()
    .map(|r| r.aggregate_score);

    let inputs = ConfluenceInputs {
        tbm_score,
        tbm_confidence,
        detections,
        onchain,
    };

    // Skip writes when there is literally nothing to score — keeps the
    // table from filling with zero-layer rows on cold starts.
    if inputs.tbm_score.is_none()
        && inputs.onchain.is_none()
        && inputs.detections.is_empty()
    {
        return Ok(false);
    }

    let reading: ConfluenceReading = score_confluence(&inputs, &cfg.weights);

    let raw_meta = json!({
        "details": reading.details,
        "weights": {
            "elliott": cfg.weights.elliott,
            "harmonic": cfg.weights.harmonic,
            "classical": cfg.weights.classical,
            "wyckoff": cfg.weights.wyckoff,
            "range": cfg.weights.range,
            "tbm": cfg.weights.tbm,
            "onchain": cfg.weights.onchain,
            "min_layers": cfg.weights.min_layers,
        },
    });

    insert_v2_confluence(
        pool,
        &V2ConfluenceInsert {
            exchange: sym.exchange.clone(),
            symbol: sym.symbol.clone(),
            timeframe: sym.interval.clone(),
            erken_uyari: reading.erken_uyari as f32,
            guven: reading.guven as f32,
            direction: reading.direction.as_str().to_string(),
            layer_count: reading.layer_count as i32,
            raw_meta,
        },
    )
    .await?;
    Ok(true)
}
