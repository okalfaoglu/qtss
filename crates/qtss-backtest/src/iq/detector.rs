//! `IqSetupDetector` — historical replay of the IQ-D / IQ-T candidate
//! pipeline (FAZ 26.2).
//!
//! At each bar, the runner asks: "given everything detected up to
//! and including this bar, would the live worker have opened a
//! setup right now?" The detector answers by:
//!
//!   1. Querying detections (motive, abc, wyckoff events, cycles)
//!      with `end_time <= bar_time` — strictly historical, no
//!      future leakage.
//!   2. Reconstructing the polarity-aware composite score (mirrors
//!      `score_*` fns from `major_dip_candidate_loop.rs`, but
//!      time-bounded).
//!   3. Comparing composite >= `gates.min_composite`; if so, looks
//!      up the latest structural extremum (last L2 pivot in the
//!      desired direction) and emits an `IqTrade` in `Pending`
//!      state.
//!
//! v1 SCOPE (this commit): a structurally-correct replay that
//! generates trades on event-confluence + cycle context. Live-parity
//! requires every component scorer (volume, fib, sentiment, ...) and
//! that lands in 26.3 once the scoring functions are refactored to
//! accept a `time_cutoff` parameter.

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{debug, trace};

use super::config::{IqBacktestConfig, IqPolarity};
use super::runner::SetupDetector;
use super::scorers::{
    cvd_divergence, fib_retrace_quality, funding_oi_signals,
    indicator_alignment, multi_tf_confluence, sentiment_extreme,
    structural_completion, volume_capitulation, ScoreKey,
};
use super::trade::IqTrade;

/// IQ-specific replay detector. Lives entirely off historical
/// detection rows — never queries any "current state" so it can
/// safely run over years of bar data.
pub struct IqReplayDetector {
    pub config: IqBacktestConfig,
}

impl IqReplayDetector {
    pub fn new(config: IqBacktestConfig) -> Self {
        Self { config }
    }
}

/// Compact wyckoff-event scoring matrix (event-only slice of the
/// live `score_wyckoff_alignment`). Returns the alignment score for
/// the given polarity + event subkind + Elliott wave context.
fn wyckoff_event_score(
    polarity: IqPolarity,
    subkind: &str,
    current_wave: &str,
) -> f64 {
    match (polarity, subkind, current_wave) {
        (IqPolarity::Dip, "spring_bull", "W2") => 1.0,
        (IqPolarity::Dip, "spring_bull", _) => 0.7,
        (IqPolarity::Dip, "sc_bull", "C") => 0.95,
        (IqPolarity::Dip, "sc_bull", _) => 0.7,
        (IqPolarity::Dip, "sos_bull", "W3") => 0.85,
        (IqPolarity::Dip, "sos_bull", _) => 0.5,
        (IqPolarity::Dip, "st_bull", "W2") => 0.7,
        (IqPolarity::Dip, "st_bull", _) => 0.4,
        (IqPolarity::Dip, "lps_bull", "W4") => 0.7,
        (IqPolarity::Dip, "lps_bull", _) => 0.4,
        (IqPolarity::Dip, "ps_bull", _) => 0.4,
        (IqPolarity::Dip, "test_bull", _) => 0.55,
        (IqPolarity::Dip, "bu_bull", _) => 0.55,
        (IqPolarity::Dip, "ar_bull", _) => 0.4,

        (IqPolarity::Top, "bc_bear", "W5") => 1.0,
        (IqPolarity::Top, "bc_bear", _) => 0.7,
        (IqPolarity::Top, "utad_bear", "B") => 0.95,
        (IqPolarity::Top, "utad_bear", _) => 0.7,
        (IqPolarity::Top, "sow_bear", "C") => 0.85,
        (IqPolarity::Top, "sow_bear", _) => 0.5,

        _ => 0.0,
    }
}

/// Mirror of `score_cycle_alignment` matrix from the live worker —
/// pure function so backtest replay matches live output bar-for-bar.
fn cycle_alignment_score(
    polarity: IqPolarity,
    phase: &str,
    source: &str,
    progress: f64,
) -> f64 {
    let base = match (polarity, phase) {
        (IqPolarity::Dip, "accumulation") => 1.0,
        (IqPolarity::Dip, "markup") => {
            if progress > 0.5 {
                0.30
            } else {
                0.55
            }
        }
        (IqPolarity::Dip, "distribution") => 0.0,
        (IqPolarity::Dip, "markdown") => 0.0,
        (IqPolarity::Top, "distribution") => 1.0,
        (IqPolarity::Top, "markdown") => {
            if progress > 0.5 {
                0.30
            } else {
                0.55
            }
        }
        (IqPolarity::Top, "accumulation") => 0.0,
        (IqPolarity::Top, "markup") => 0.0,
        _ => 0.0,
    };
    let source_w = match source {
        "confluent" => 1.0,
        "elliott" => 0.85,
        "event" => 0.65,
        _ => 0.5,
    };
    base * source_w
}

#[async_trait::async_trait]
impl SetupDetector for IqReplayDetector {
    async fn detect_at(
        &self,
        pool: &PgPool,
        bar_index: usize,
        bar_time: DateTime<Utc>,
        bar_close: Decimal,
    ) -> Vec<IqTrade> {
        let cfg = &self.config;
        let u = &cfg.universe;

        // ── 1) Latest Wyckoff event for (sym, tf), end_time <= bar_time
        let wy_row = sqlx::query(
            r#"SELECT subkind, raw_meta
                 FROM detections
                WHERE exchange=$1 AND segment=$2 AND symbol=$3
                  AND timeframe=$4
                  AND pattern_family='wyckoff' AND mode='live'
                  AND invalidated=false
                  AND subkind NOT LIKE 'cycle_%'
                  AND subkind NOT LIKE 'range_%'
                  AND end_time <= $5
                ORDER BY end_time DESC LIMIT 1"#,
        )
        .bind(&u.exchange).bind(&u.segment).bind(&u.symbol).bind(&u.timeframe)
        .bind(bar_time)
        .fetch_optional(pool).await.ok().flatten();

        let (event_subkind, event_score) = match wy_row {
            Some(row) => {
                let sk: String = row.try_get("subkind").unwrap_or_default();
                let _meta: Value = row.try_get("raw_meta").unwrap_or(Value::Null);
                let score = wyckoff_event_score(cfg.polarity, &sk, "");
                (Some(sk), score)
            }
            None => (None, 0.0),
        };

        // ── 2) Active cycle tile covering bar_time, polarity-aware.
        let tile = sqlx::query(
            r#"SELECT subkind, raw_meta, start_time, end_time
                 FROM detections
                WHERE exchange=$1 AND segment=$2 AND symbol=$3
                  AND timeframe=$4
                  AND pattern_family='wyckoff' AND mode='live'
                  AND subkind LIKE 'cycle_%'
                  AND invalidated=false
                  AND start_time <= $5
                ORDER BY end_time DESC LIMIT 1"#,
        )
        .bind(&u.exchange).bind(&u.segment).bind(&u.symbol).bind(&u.timeframe)
        .bind(bar_time)
        .fetch_optional(pool).await.ok().flatten();

        let (cycle_phase, cycle_source, cycle_score) = match tile {
            Some(row) => {
                let subkind: String = row.try_get("subkind").unwrap_or_default();
                let phase = subkind.replace("cycle_", "");
                let raw: Value = row.try_get("raw_meta").unwrap_or(Value::Null);
                let source = raw
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("event")
                    .to_string();
                let start_t: DateTime<Utc> = row
                    .try_get("start_time")
                    .unwrap_or(bar_time);
                let end_t: DateTime<Utc> =
                    row.try_get("end_time").unwrap_or(bar_time);
                let total = (end_t - start_t).num_seconds().max(1) as f64;
                let elapsed = (bar_time - start_t).num_seconds().max(0) as f64;
                let progress = (elapsed / total).clamp(0.0, 1.0);
                let score =
                    cycle_alignment_score(cfg.polarity, &phase, &source, progress);
                (Some(phase), Some(source), score)
            }
            None => (None, None, 0.0),
        };

        // ── 3) Compute the remaining 8 component scores (FAZ 26.3 —
        // live parity). Each scorer accepts bar_time and queries
        // strictly historical data with a "<=" cutoff so the
        // backtest never sees future bars. When a data source is
        // missing for the bar (e.g. the symbol predates indicator
        // ingestion), the scorer returns 0.0 — same neutral default
        // the live worker uses.
        let key = ScoreKey {
            exchange: &u.exchange,
            segment: &u.segment,
            symbol: &u.symbol,
            timeframe: &u.timeframe,
        };
        let c_struct = structural_completion(pool, &key, bar_time).await;
        let c_fib =
            fib_retrace_quality(pool, &key, bar_time, bar_close).await;
        let c_volume =
            volume_capitulation(pool, &key, bar_time, cfg.polarity).await;
        let c_cvd = cvd_divergence(pool, &key, bar_time, cfg.polarity).await;
        let c_indicator =
            indicator_alignment(pool, &key, bar_time, cfg.polarity).await;
        let c_sentiment = sentiment_extreme(pool, bar_time, cfg.polarity).await;
        let c_multi_tf =
            multi_tf_confluence(pool, &key, bar_time, cfg.polarity).await;
        let c_funding =
            funding_oi_signals(pool, &key, bar_time, cfg.polarity).await;

        let w = &cfg.weights;
        let composite = w.structural * c_struct
            + w.fib_retrace * c_fib
            + w.volume_capit * c_volume
            + w.cvd_divergence * c_cvd
            + w.indicator * c_indicator
            + w.sentiment * c_sentiment
            + w.multi_tf * c_multi_tf
            + w.funding_oi * c_funding
            + w.wyckoff_alignment * event_score
            + w.cycle_alignment * cycle_score;
        let composite = composite.clamp(0.0, 1.0);

        // Cycle veto — if cycle phase contradicts polarity, skip
        // entirely (matches live `require_cycle_alignment=true`).
        if cfg.gates.require_cycle_alignment {
            if let Some(ph) = &cycle_phase {
                let veto = match cfg.polarity {
                    IqPolarity::Dip => {
                        ph == "distribution" || ph == "markdown"
                    }
                    IqPolarity::Top => {
                        ph == "accumulation" || ph == "markup"
                    }
                };
                if veto {
                    trace!(
                        bar = bar_index,
                        polarity = ?cfg.polarity,
                        phase = ph,
                        "cycle veto"
                    );
                    return Vec::new();
                }
            }
        }

        if composite < cfg.gates.min_composite {
            trace!(
                bar = bar_index,
                composite,
                threshold = cfg.gates.min_composite,
                "composite below threshold"
            );
            return Vec::new();
        }

        // ── 4) Build a trade. Stop loss = 1% below entry for Dip,
        // 1% above for Top (placeholder — 26.3 will read structural
        // invalidation from the originating motive's W0 / X0). TPs
        // = 1×, 2×, 3× the SL distance (1R / 2R / 3R ladder).
        let entry_price = bar_close;
        let entry_f = entry_price.to_f64().unwrap_or(0.0);
        let (sl, tps) = match cfg.polarity {
            IqPolarity::Dip => {
                let sl = Decimal::from_f64_retain(entry_f * 0.99)
                    .unwrap_or(entry_price);
                let tp1 = Decimal::from_f64_retain(entry_f * 1.01)
                    .unwrap_or(entry_price);
                let tp2 = Decimal::from_f64_retain(entry_f * 1.02)
                    .unwrap_or(entry_price);
                let tp3 = Decimal::from_f64_retain(entry_f * 1.03)
                    .unwrap_or(entry_price);
                (sl, vec![tp1, tp2, tp3])
            }
            IqPolarity::Top => {
                let sl = Decimal::from_f64_retain(entry_f * 1.01)
                    .unwrap_or(entry_price);
                let tp1 = Decimal::from_f64_retain(entry_f * 0.99)
                    .unwrap_or(entry_price);
                let tp2 = Decimal::from_f64_retain(entry_f * 0.98)
                    .unwrap_or(entry_price);
                let tp3 = Decimal::from_f64_retain(entry_f * 0.97)
                    .unwrap_or(entry_price);
                (sl, vec![tp1, tp2, tp3])
            }
        };

        // Position sizing — risk_per_trade_pct of starting equity,
        // distance to SL determines qty.
        let risk_quote = cfg.risk.starting_equity
            * Decimal::from_f64_retain(cfg.risk.risk_per_trade_pct)
                .unwrap_or(Decimal::ZERO);
        let sl_distance = (entry_price - sl).abs();
        let qty = if sl_distance.is_zero() {
            Decimal::ZERO
        } else {
            risk_quote / sl_distance
        };
        if qty <= Decimal::ZERO {
            return Vec::new();
        }

        let components = json!({
            "structural_completion": c_struct,
            "fib_retrace_quality":   c_fib,
            "volume_capitulation":   c_volume,
            "cvd_divergence":        c_cvd,
            "indicator_alignment":   c_indicator,
            "sentiment_extreme":     c_sentiment,
            "multi_tf_confluence":   c_multi_tf,
            "funding_oi_signals":    c_funding,
            "wyckoff_alignment":     event_score,
            "wyckoff_event":         event_subkind,
            "cycle_alignment":       cycle_score,
            "cycle_phase":           cycle_phase,
            "cycle_source":          cycle_source,
        });

        let mut trade = IqTrade::pending(
            cfg.run_tag.clone(),
            cfg.polarity,
            u.symbol.clone(),
            u.timeframe.clone(),
            u.exchange.clone(),
            u.segment.clone(),
            bar_index,
            bar_time,
            entry_price,
            sl,
            tps,
            qty,
            components,
            composite,
        );
        trade.wyckoff_event_at_entry = event_subkind.clone();
        trade.cycle_phase_at_entry = cycle_phase.clone();
        trade.cycle_source_at_entry = cycle_source.clone();
        // Confirm into Open — v1 fires entries at the close of the
        // signal bar (no next-bar delay). Future variant: configurable
        // entry_mode = ClosingBar | NextOpen.
        trade.state = super::trade::TradeState::Open;

        debug!(
            bar = bar_index,
            polarity = ?cfg.polarity,
            composite,
            event = ?event_subkind,
            phase = ?cycle_phase,
            "iq replay: setup opened"
        );
        vec![trade]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iq::config::IqBacktestConfig;
    use crate::iq::config::IqPolarity;

    #[test]
    fn wyckoff_event_score_matrix_pruden_aligned() {
        // Spring + W2 = 1.0 (textbook accumulation Phase C launch).
        assert!((wyckoff_event_score(IqPolarity::Dip, "spring_bull", "W2") - 1.0).abs() < 0.001);
        // BC + W5 = 1.0 (blowoff distribution).
        assert!((wyckoff_event_score(IqPolarity::Top, "bc_bear", "W5") - 1.0).abs() < 0.001);
        // Cross-polarity = 0.0 (active veto).
        assert!(wyckoff_event_score(IqPolarity::Dip, "bc_bear", "W5") < 0.001);
    }

    #[test]
    fn cycle_alignment_dip_in_distribution_is_zero() {
        let s = cycle_alignment_score(IqPolarity::Dip, "distribution", "elliott", 0.3);
        assert!(s < 0.001);
    }

    #[test]
    fn cycle_alignment_top_in_distribution_max_when_confluent() {
        let s = cycle_alignment_score(IqPolarity::Top, "distribution", "confluent", 0.3);
        assert!((s - 1.0).abs() < 0.001);
    }

    #[test]
    fn cycle_alignment_late_markup_dampened() {
        let early = cycle_alignment_score(IqPolarity::Dip, "markup", "elliott", 0.2);
        let late = cycle_alignment_score(IqPolarity::Dip, "markup", "elliott", 0.8);
        assert!(early > late);
    }

    #[test]
    fn replay_detector_construct() {
        let cfg = IqBacktestConfig::default();
        let _d = IqReplayDetector::new(cfg);
    }
}
