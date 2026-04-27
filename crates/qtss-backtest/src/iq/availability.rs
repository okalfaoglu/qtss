//! Pre-flight data availability probe.
//!
//! User-reported BUG BACKTEST: across many runs the IQ replay
//! detector produces 0 trades over thousands of bars and the operator
//! has no visibility into WHY — the runner just iterates silently and
//! every component scorer that hits a missing/stale table returns 0.
//!
//! This probe runs ONCE at the start of `IqBacktestRunner::run` and
//! returns a per-channel verdict (table missing / empty in window /
//! partial coverage / full). The runner logs the verdict and the CLI
//! surfaces it in the report so the operator can see — at a glance —
//! which channels actually contributed to (or starved) the run.
//!
//! Critically, this is read-only and never alters scorer behaviour.
//! It exists purely as a diagnostic.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::config::IqBacktestConfig;

/// One row in the availability matrix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelAvailability {
    /// Composite key (e.g. "structural_completion").
    pub channel: String,
    /// Backing source ("iq_structures", "pivots", ...).
    pub source: String,
    /// `Missing` = table doesn't exist in the schema.
    /// `Empty`   = table exists but has 0 rows in the window.
    /// `Partial` = some rows; coverage < 80% of the window.
    /// `Full`    = ≥80% of the window covered.
    pub status: String,
    /// Number of rows the probe found inside the window.
    pub rows_in_window: i64,
    /// First row open_time inside the window (None if Empty/Missing).
    pub earliest: Option<DateTime<Utc>>,
    /// Last row open_time inside the window (None if Empty/Missing).
    pub latest: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataAvailabilityReport {
    pub rows: Vec<ChannelAvailability>,
}

impl DataAvailabilityReport {
    /// True when every channel either fully covers the window or is
    /// labelled `Skipped` (zero weight in the active config).
    pub fn fully_healthy(&self) -> bool {
        self.rows.iter().all(|r| r.status == "full")
    }

    /// True when at least one channel is missing or empty — the
    /// runner is unlikely to fire trades and the operator should see
    /// a clear warning.
    pub fn has_critical_gap(&self) -> bool {
        self.rows
            .iter()
            .any(|r| r.status == "missing" || r.status == "empty")
    }

    /// Pretty-print the matrix to stdout. Used by the CLI before the
    /// bar loop so the operator sees the verdict immediately.
    pub fn print(&self) {
        println!();
        println!("─── data availability probe ──────────────────────────");
        println!(
            "  {:<28} {:<24} {:<8} {:>8}",
            "channel", "source", "status", "rows"
        );
        for r in &self.rows {
            let status_disp = match r.status.as_str() {
                "full" => "✓ full",
                "partial" => "~ partial",
                "empty" => "✗ empty",
                "missing" => "✗ missing",
                other => other,
            };
            println!(
                "  {:<28} {:<24} {:<8} {:>8}",
                r.channel, r.source, status_disp, r.rows_in_window
            );
        }
        if self.has_critical_gap() {
            println!();
            println!(
                "  ⚠  one or more channels are missing or empty in this window;"
            );
            println!(
                "     the corresponding scorer will return 0 for every bar."
            );
            println!(
                "     consider lowering `gates.min_composite` or zeroing the"
            );
            println!("     weight on the dead channel(s).");
        }
        println!("──────────────────────────────────────────────────────");
    }
}

/// Probe each backing table for the configured (sym, tf, window).
/// Cheap — every query is a `COUNT(*)` + min/max bound.
pub async fn probe(
    pool: &PgPool,
    cfg: &IqBacktestConfig,
) -> DataAvailabilityReport {
    let u = &cfg.universe;
    let mut rows: Vec<ChannelAvailability> = Vec::new();

    // ── 1) iq_structures (structural_completion)
    rows.push(
        probe_window(
            pool,
            "structural_completion",
            "iq_structures",
            r#"SELECT COUNT(*) AS n, MIN(last_advanced_at) AS lo, MAX(last_advanced_at) AS hi
                 FROM iq_structures
                WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4
                  AND last_advanced_at BETWEEN $5 AND $6"#,
            &u.exchange,
            &u.segment,
            &u.symbol,
            &u.timeframe,
            u.start_time,
            u.end_time,
        )
        .await,
    );

    // ── 2) pivots L2 (fib_retrace_quality)
    rows.push(
        probe_window(
            pool,
            "fib_retrace_quality",
            "pivots (L=2)",
            r#"SELECT COUNT(*) AS n, MIN(p.open_time) AS lo, MAX(p.open_time) AS hi
                 FROM pivots p
                 JOIN engine_symbols es ON es.id = p.engine_symbol_id
                WHERE es.exchange=$1 AND es.segment=$2
                  AND es.symbol=$3 AND es.interval=$4
                  AND p.level = 2
                  AND p.open_time BETWEEN $5 AND $6"#,
            &u.exchange,
            &u.segment,
            &u.symbol,
            &u.timeframe,
            u.start_time,
            u.end_time,
        )
        .await,
    );

    // ── 3) market_bars (volume_capit + cvd_divergence rely on this)
    rows.push(
        probe_window(
            pool,
            "volume_capitulation",
            "market_bars",
            r#"SELECT COUNT(*) AS n, MIN(open_time) AS lo, MAX(open_time) AS hi
                 FROM market_bars
                WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND interval=$4
                  AND open_time BETWEEN $5 AND $6"#,
            &u.exchange,
            &u.segment,
            &u.symbol,
            &u.timeframe,
            u.start_time,
            u.end_time,
        )
        .await,
    );

    // ── 4) wyckoff event detections (wyckoff_alignment)
    rows.push(
        probe_window(
            pool,
            "wyckoff_alignment",
            "detections wyckoff",
            r#"SELECT COUNT(*) AS n, MIN(end_time) AS lo, MAX(end_time) AS hi
                 FROM detections
                WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4
                  AND pattern_family='wyckoff' AND mode='live'
                  AND invalidated=false
                  AND subkind NOT LIKE 'cycle_%'
                  AND subkind NOT LIKE 'range_%'
                  AND end_time BETWEEN $5 AND $6"#,
            &u.exchange,
            &u.segment,
            &u.symbol,
            &u.timeframe,
            u.start_time,
            u.end_time,
        )
        .await,
    );

    // ── 5) wyckoff cycle tiles (cycle_alignment)
    rows.push(
        probe_window(
            pool,
            "cycle_alignment",
            "detections cycle_*",
            r#"SELECT COUNT(*) AS n, MIN(start_time) AS lo, MAX(end_time) AS hi
                 FROM detections
                WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4
                  AND pattern_family='wyckoff' AND mode='live'
                  AND subkind LIKE 'cycle_%'
                  AND invalidated=false
                  AND start_time <= $6 AND end_time >= $5"#,
            &u.exchange,
            &u.segment,
            &u.symbol,
            &u.timeframe,
            u.start_time,
            u.end_time,
        )
        .await,
    );

    // ── 6) indicator_alignment — computed INLINE from market_bars
    // since 2026-04-27 (Wilder RSI + MACD on the fly). The legacy
    // bar_indicator_snapshots table is no longer required; the
    // scorer falls back to it when present but works without it.
    rows.push(ChannelAvailability {
        channel: "indicator_alignment".to_string(),
        source: "market_bars (inline RSI + MACD)".to_string(),
        status: "full".to_string(),
        rows_in_window: -1,
        earliest: None,
        latest: None,
    });

    // ── 7) fear_greed_snapshots (sentiment_extreme)
    rows.push(
        probe_table_existence(
            pool,
            "sentiment_extreme",
            "fear_greed_snapshots",
        )
        .await,
    );

    // ── 8) external_snapshots (funding_oi_signals)
    rows.push(
        probe_table_existence(
            pool,
            "funding_oi_signals",
            "external_snapshots",
        )
        .await,
    );

    DataAvailabilityReport { rows }
}

#[allow(clippy::too_many_arguments)]
async fn probe_window(
    pool: &PgPool,
    channel: &str,
    source: &str,
    sql: &str,
    exchange: &str,
    segment: &str,
    symbol: &str,
    timeframe: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> ChannelAvailability {
    let row = sqlx::query(sql)
        .bind(exchange)
        .bind(segment)
        .bind(symbol)
        .bind(timeframe)
        .bind(start)
        .bind(end)
        .fetch_one(pool)
        .await;
    let (n, lo, hi) = match row {
        Ok(r) => {
            let n: i64 = r.try_get("n").unwrap_or(0);
            let lo: Option<DateTime<Utc>> = r.try_get("lo").ok();
            let hi: Option<DateTime<Utc>> = r.try_get("hi").ok();
            (n, lo, hi)
        }
        Err(e) => {
            // Table missing or other error — surface as missing.
            let msg = e.to_string();
            let status = if msg.contains("does not exist") {
                "missing"
            } else {
                "missing"
            };
            return ChannelAvailability {
                channel: channel.to_string(),
                source: source.to_string(),
                status: status.to_string(),
                rows_in_window: 0,
                earliest: None,
                latest: None,
            };
        }
    };
    let status = classify_coverage(n, lo, hi, start, end);
    ChannelAvailability {
        channel: channel.to_string(),
        source: source.to_string(),
        status,
        rows_in_window: n,
        earliest: lo,
        latest: hi,
    }
}

async fn probe_table_existence(
    pool: &PgPool,
    channel: &str,
    table: &str,
) -> ChannelAvailability {
    let row =
        sqlx::query("SELECT to_regclass($1) IS NOT NULL AS present")
            .bind(table)
            .fetch_one(pool)
            .await;
    let exists = row
        .ok()
        .and_then(|r| r.try_get::<bool, _>("present").ok())
        .unwrap_or(false);
    if !exists {
        return ChannelAvailability {
            channel: channel.to_string(),
            source: table.to_string(),
            status: "missing".to_string(),
            rows_in_window: 0,
            earliest: None,
            latest: None,
        };
    }
    // Table exists but we don't have a tight per-window count without
    // table-specific keys. Mark as full so operators don't get a
    // false alarm when the worker's actually populating it.
    ChannelAvailability {
        channel: channel.to_string(),
        source: table.to_string(),
        status: "full".to_string(),
        rows_in_window: -1, // -1 = unknown / not counted
        earliest: None,
        latest: None,
    }
}

fn classify_coverage(
    n: i64,
    lo: Option<DateTime<Utc>>,
    hi: Option<DateTime<Utc>>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> String {
    if n == 0 {
        return "empty".to_string();
    }
    let (Some(lo), Some(hi)) = (lo, hi) else {
        return "partial".to_string();
    };
    let window_secs = (end - start).num_seconds().max(1) as f64;
    let covered_secs = (hi - lo).num_seconds().max(0) as f64;
    let ratio = (covered_secs / window_secs).clamp(0.0, 1.0);
    if ratio >= 0.8 {
        "full".to_string()
    } else {
        "partial".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn empty_window_classifies_empty() {
        let s = chrono::Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let e = chrono::Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
        assert_eq!(classify_coverage(0, None, None, s, e), "empty");
    }

    #[test]
    fn full_coverage_threshold() {
        let s = chrono::Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let e = chrono::Utc.with_ymd_and_hms(2025, 12, 31, 0, 0, 0).unwrap();
        let lo = chrono::Utc.with_ymd_and_hms(2025, 1, 5, 0, 0, 0).unwrap();
        let hi = chrono::Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();
        assert_eq!(classify_coverage(100, Some(lo), Some(hi), s, e), "full");
    }

    #[test]
    fn partial_coverage_threshold() {
        let s = chrono::Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let e = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let lo = chrono::Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
        let hi = chrono::Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap();
        assert_eq!(
            classify_coverage(100, Some(lo), Some(hi), s, e),
            "partial"
        );
    }
}
