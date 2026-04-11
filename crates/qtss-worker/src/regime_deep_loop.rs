//! Faz 11 — Regime Deep worker loop.
//!
//! Every tick:
//! 1. For each enabled symbol × configured timeframes → compute regime via RegimeEngine.
//! 2. Insert regime_snapshots row.
//! 3. Compare with previous snapshot → detect transitions.
//! 4. Insert transition row + optional Telegram alert.
//! 5. Purge old snapshots beyond retention window.

use chrono::Utc;
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};
use qtss_domain::v2::timeframe::Timeframe;
use qtss_regime::{RegimeConfig, RegimeEngine};
use qtss_storage::{
    insert_regime_snapshot, insert_regime_transition, latest_regime_snapshot,
    market_bars, purge_old_snapshots, resolve_system_f64, resolve_system_string,
    resolve_system_u64, resolve_worker_enabled_flag, resolve_worker_tick_secs,
    RegimeSnapshotInsert, RegimeTransitionInsert,
};
use rust_decimal::prelude::ToPrimitive;
use sqlx::PgPool;
use std::time::Duration;
use tracing::{debug, info, warn};

pub async fn regime_deep_loop(pool: PgPool) {
    info!("regime_deep_loop started");

    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool, "worker", "regime_deep_enabled", "QTSS_REGIME_DEEP_ENABLED", true,
        ).await;
        let tick_secs = resolve_worker_tick_secs(
            &pool, "regime", "snapshot_tick_secs", "QTSS_REGIME_TICK_SECS", 300, 60,
        ).await;

        if !enabled {
            tokio::time::sleep(Duration::from_secs(tick_secs)).await;
            continue;
        }

        if let Err(e) = regime_deep_tick(&pool).await {
            warn!(%e, "regime_deep_tick error");
        }

        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}

async fn regime_deep_tick(pool: &PgPool) -> anyhow::Result<()> {
    // Resolve config
    let timeframes_str = resolve_system_string(
        pool, "regime", "timeframes", "", r#"["15m","1h","4h","1d"]"#,
    ).await;
    let timeframes: Vec<String> = serde_json::from_str(&timeframes_str)
        .unwrap_or_else(|_| vec!["15m".into(), "1h".into(), "4h".into(), "1d".into()]);

    let transition_enabled = resolve_system_string(
        pool, "regime", "transition_detection_enabled", "", "true",
    ).await == "true";
    let transition_min_conf = resolve_system_f64(
        pool, "regime", "transition_min_confidence", "", 0.6,
    ).await;
    let retention_days = resolve_system_u64(
        pool, "regime", "snapshot_retention_days", "", 7, 1, 365,
    ).await;

    let regime_cfg = resolve_regime_config(pool).await;

    // Get all enabled engine symbols
    let symbols = qtss_storage::list_enabled_engine_symbols(pool).await?;

    let mut snap_count = 0u32;
    let mut trans_count = 0u32;

    for sym_row in &symbols {
        let symbol = &sym_row.symbol;
        let venue = &sym_row.exchange;
        let segment = &sym_row.segment;

        for tf in &timeframes {
            match compute_and_store(
                pool, venue, segment, symbol, tf, &regime_cfg,
                transition_enabled, transition_min_conf,
            ).await {
                Ok((snapped, transitioned)) => {
                    if snapped { snap_count += 1; }
                    if transitioned { trans_count += 1; }
                }
                Err(e) => {
                    debug!(%e, symbol, tf, "regime compute skip");
                }
            }
        }
    }

    // Purge old snapshots
    if retention_days > 0 {
        let purged = purge_old_snapshots(pool, retention_days as i64).await.unwrap_or(0);
        if purged > 0 {
            debug!(purged, "regime snapshots purged");
        }
    }

    if snap_count > 0 {
        info!(snapshots = snap_count, transitions = trans_count, "regime_deep_tick done");
    }

    Ok(())
}

async fn compute_and_store(
    pool: &PgPool,
    venue: &str,
    segment: &str,
    symbol: &str,
    interval: &str,
    regime_cfg: &RegimeConfig,
    transition_enabled: bool,
    transition_min_conf: f64,
) -> anyhow::Result<(bool, bool)> {
    // Fetch recent bars
    let window = 200i64;
    let mut rows = market_bars::list_recent_bars(pool, venue, segment, symbol, interval, window).await?;
    if rows.len() < 50 {
        return Ok((false, false));
    }
    rows.reverse(); // chronological

    let timeframe = parse_tf(interval).ok_or_else(|| anyhow::anyhow!("bad tf: {interval}"))?;
    let instrument = placeholder_instrument(venue, symbol);

    let mut engine = RegimeEngine::new(regime_cfg.clone())
        .map_err(|e| anyhow::anyhow!("regime engine: {e}"))?;

    let mut last_snap: Option<RegimeSnapshot> = None;
    for r in &rows {
        let bar = Bar {
            instrument: instrument.clone(),
            timeframe,
            open_time: r.open_time,
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume,
            closed: true,
        };
        if let Ok(Some(s)) = engine.on_bar(&bar) {
            last_snap = Some(s);
        }
    }

    let snap = match last_snap {
        Some(s) => s,
        None => return Ok((false, false)),
    };

    // Insert snapshot
    let adx_f = snap.adx.to_f64().unwrap_or(0.0);
    let bb_f = snap.bb_width.to_f64().unwrap_or(0.0);
    let atr_f = snap.atr_pct.to_f64().unwrap_or(0.0);
    let chop_f = snap.choppiness.to_f64().unwrap_or(0.0);

    let insert = RegimeSnapshotInsert {
        symbol: symbol.to_string(),
        interval: interval.to_string(),
        regime: snap.kind.as_str().to_string(),
        trend_strength: Some(snap.trend_strength.as_str().to_string()),
        confidence: snap.confidence as f64,
        adx: Some(adx_f),
        plus_di: None, // engine doesn't expose +DI/-DI separately yet
        minus_di: None,
        bb_width: Some(bb_f),
        atr_pct: Some(atr_f),
        choppiness: Some(chop_f),
        hmm_state: None,
        hmm_confidence: None,
    };
    insert_regime_snapshot(pool, &insert).await?;

    // Transition detection
    let mut transitioned = false;
    if transition_enabled {
        if let Ok(Some(prev_row)) = latest_regime_snapshot(pool, symbol, interval).await {
            // Build a RegimeSnapshot from previous row
            if let Some(prev_kind) = RegimeKind::from_str_opt(&prev_row.regime) {
                if prev_kind != snap.kind {
                    let prev_snap = RegimeSnapshot {
                        at: prev_row.computed_at,
                        kind: prev_kind,
                        trend_strength: prev_row.trend_strength.as_deref()
                            .and_then(TrendStrength::from_str_opt)
                            .unwrap_or(TrendStrength::None),
                        adx: rust_decimal::Decimal::from_f64_retain(prev_row.adx.unwrap_or(0.0)).unwrap_or_default(),
                        bb_width: rust_decimal::Decimal::from_f64_retain(prev_row.bb_width.unwrap_or(0.0)).unwrap_or_default(),
                        atr_pct: rust_decimal::Decimal::from_f64_retain(prev_row.atr_pct.unwrap_or(0.0)).unwrap_or_default(),
                        choppiness: rust_decimal::Decimal::from_f64_retain(prev_row.choppiness.unwrap_or(0.0)).unwrap_or_default(),
                        confidence: prev_row.confidence as f32,
                    };

                    if let Some(t) = qtss_regime::transition::detect_transition(
                        symbol, interval, &prev_snap, &snap, transition_min_conf,
                    ) {
                        let ti = RegimeTransitionInsert {
                            symbol: symbol.to_string(),
                            interval: interval.to_string(),
                            from_regime: t.from_regime.as_str().to_string(),
                            to_regime: t.to_regime.as_str().to_string(),
                            transition_speed: Some(t.transition_speed),
                            confidence: t.confidence,
                            confirming_indicators: t.confirming_indicators,
                            hmm_probability: None,
                        };
                        insert_regime_transition(pool, &ti).await?;
                        transitioned = true;
                        info!(
                            symbol, interval,
                            from = t.from_regime.as_str(),
                            to = t.to_regime.as_str(),
                            confidence = t.confidence,
                            "regime transition detected"
                        );
                    }
                }
            }
        }
    }

    Ok((true, transitioned))
}

async fn resolve_regime_config(pool: &PgPool) -> RegimeConfig {
    let adx_period = resolve_system_u64(pool, "regime", "adx_period", "", 14, 2, 100).await as usize;
    let bb_period = resolve_system_u64(pool, "regime", "bb_period", "", 20, 2, 200).await as usize;
    let bb_stddev = resolve_system_f64(pool, "regime", "bb_stddev", "", 2.0).await;
    let chop_period = resolve_system_u64(pool, "regime", "chop_period", "", 14, 2, 100).await as usize;
    let adx_trend_threshold = resolve_system_f64(pool, "regime", "adx_trend_threshold", "", 25.0).await;
    let adx_strong_threshold = resolve_system_f64(pool, "regime", "adx_strong_threshold", "", 40.0).await;
    let bb_squeeze_threshold = resolve_system_f64(pool, "regime", "bb_squeeze_threshold", "", 0.05).await;
    let volatility_threshold = resolve_system_f64(pool, "regime", "volatility_threshold", "", 0.04).await;
    let chop_range_threshold = resolve_system_f64(pool, "regime", "chop_range_threshold", "", 61.8).await;

    RegimeConfig {
        adx_period,
        bb_period,
        bb_stddev,
        chop_period,
        adx_trend_threshold,
        adx_strong_threshold,
        bb_squeeze_threshold,
        volatility_threshold,
        chop_range_threshold,
    }
}

fn placeholder_instrument(venue: &str, symbol: &str) -> Instrument {
    use rust_decimal::Decimal;
    let v = match venue.to_lowercase().as_str() {
        "binance" => Venue::Binance,
        other => Venue::Custom(other.to_string()),
    };
    Instrument {
        venue: v,
        asset_class: AssetClass::CryptoSpot,
        symbol: symbol.to_string(),
        quote_ccy: "USDT".to_string(),
        tick_size: Decimal::new(1, 8),
        lot_size: Decimal::new(1, 8),
        session: SessionCalendar::binance_24x7(),
    }
}

fn parse_tf(interval: &str) -> Option<Timeframe> {
    match interval.trim().to_lowercase().as_str() {
        "1m" => Some(Timeframe::M1),
        "5m" => Some(Timeframe::M5),
        "15m" => Some(Timeframe::M15),
        "30m" => Some(Timeframe::M30),
        "1h" => Some(Timeframe::H1),
        "4h" => Some(Timeframe::H4),
        "1d" => Some(Timeframe::D1),
        _ => None,
    }
}
