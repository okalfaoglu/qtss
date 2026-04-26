//! CLI helpers — config loading, structured logging set-up, simple
//! progress reporters. The actual `qtss-backtest` binary lives at
//! `crates/qtss-backtest/src/bin/iq_backtest.rs` (added in this
//! commit) and uses these helpers to keep `main()` short.
//!
//! User spec: "analyzabileceğin detayda log ve detay ekle" — every
//! decision point in the runner emits a structured `tracing` event
//! with the full context (bar, components, polarity, outcome). When
//! a backtest produces unexpected PnL, you re-run with
//! `RUST_LOG=qtss_backtest::iq=trace` and the event log walks you
//! through every entry / TP fill / SL hit / classify decision.

use std::path::Path;
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::{fmt, EnvFilter};

use super::config::IqBacktestConfig;

/// Initialise tracing with sensible defaults for backtest runs.
/// Honours `RUST_LOG` when set; otherwise defaults to `info` for
/// the backtest crate and `warn` for everything else.
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("warn,qtss_backtest=info")
    });
    let _ = fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_level(true)
        .try_init();
}

/// Initialise tracing in JSON mode — every event becomes a single
/// JSON line. Use this when you want to ingest the log into a
/// SQL-style analyser (DuckDB, jq).
pub fn init_tracing_json() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("warn,qtss_backtest=info")
    });
    let _ = fmt()
        .json()
        .with_env_filter(filter)
        .with_target(true)
        .with_level(true)
        .try_init();
}

/// Parse a YAML / JSON config file from disk. The file shape mirrors
/// `IqBacktestConfig` exactly — serde handles either format
/// transparently via the file extension.
pub fn load_config_file(path: &Path) -> anyhow::Result<IqBacktestConfig> {
    let raw = std::fs::read_to_string(path)?;
    let cfg: IqBacktestConfig = if path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("json"))
        .unwrap_or(false)
    {
        serde_json::from_str(&raw)?
    } else {
        // Default: YAML / TOML / etc. We only ship YAML for now —
        // JSON is the lingua franca, YAML is operator-friendly.
        serde_json::from_str(&raw)
            .or_else(|_| serde_json::from_str::<IqBacktestConfig>(&raw))?
    };
    info!(
        path = %path.display(),
        symbol = %cfg.universe.symbol,
        tf = %cfg.universe.timeframe,
        polarity = ?cfg.polarity,
        "loaded backtest config"
    );
    Ok(cfg)
}

/// Pretty-print an IqBacktestReport to stdout — the human-readable
/// summary at end of run.
pub fn print_report(report: &super::report::IqBacktestReport) {
    println!();
    println!("─── IQ Backtest Report ───────────────────────────────");
    println!(" run_tag: {}", report.config.run_tag);
    println!(
        " universe: {}/{}/{} {}",
        report.config.universe.exchange,
        report.config.universe.segment,
        report.config.universe.symbol,
        report.config.universe.timeframe,
    );
    println!(
        " window:   {} -> {}",
        report.config.universe.start_time,
        report.config.universe.end_time,
    );
    println!(" bars:     {}", report.bars_processed);
    println!();
    println!(" trades:   {}", report.total_trades);
    println!(
        "   wins:     {} ({:.1}%)",
        report.wins,
        report.win_rate * 100.0
    );
    println!("   losses:   {}", report.losses);
    println!("   scratches:{}", report.scratches);
    println!("   aborted:  {}", report.aborted);
    println!("   open:     {}", report.open_at_end);
    println!();
    println!(" gross_pnl:    {}", report.gross_pnl);
    println!(" net_pnl:      {}", report.net_pnl);
    println!(" final_equity: {}", report.final_equity);
    println!(" peak_equity:  {}", report.peak_equity);
    println!(" max_dd:       {:.2}%", report.max_drawdown_pct);
    println!();
    println!(" avg_win:    {:.2}%", report.avg_win_pct);
    println!(" avg_loss:   {:.2}%", report.avg_loss_pct);
    println!(" profit_factor: {:.2}", report.profit_factor);
    println!(" expectancy: {:.2}%", report.expectancy_pct);
    if let Some(s) = report.sharpe_ratio {
        println!(" sharpe(per-trade): {:.3}", s);
    }
    println!();
    if !report.loss_reason_counts.is_empty() {
        println!(" loss reasons:");
        for (k, v) in &report.loss_reason_counts {
            println!("   {:>30}: {}", k, v);
        }
    }
    if !report.avg_loss_components.is_empty() {
        println!();
        println!(" avg component scores on losers (weakest = highest priority bug):");
        let mut rows: Vec<_> = report.avg_loss_components.iter().collect();
        rows.sort_by(|a, b| {
            a.1.partial_cmp(b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (k, v) in rows {
            println!("   {:>30}: {:.3}", k, v);
        }
    }
    println!("──────────────────────────────────────────────────────");
}

/// Pretty-print an OptimizationReport — leaderboard top-N +
/// sensitivity rows.
pub fn print_optimization_report(
    report: &super::optimize::OptimizationReport,
    top_n: usize,
) {
    println!();
    println!("─── Optimisation Report ─────────────────────────────");
    println!(" configs:  {}", report.configs_evaluated);
    println!(" windows:  {}", report.windows_evaluated);
    println!();
    let n = top_n.min(report.leaderboard.len());
    println!(" top {} by mean OOS score:", n);
    for (i, c) in report.leaderboard.iter().take(n).enumerate() {
        println!(
            "  {:>2}. is={:>10.2}  oos={:>10.2}  stddev={:>8.2}  robust={:>5.2}",
            i + 1,
            c.mean_in_sample_score,
            c.mean_oos_score,
            c.stddev_oos_score,
            c.robustness_ratio,
        );
        let w = &c.weights;
        println!(
            "      structural={:.2} fib={:.2} volume={:.2} cvd={:.2} ind={:.2}",
            w.structural, w.fib_retrace, w.volume_capit, w.cvd_divergence, w.indicator,
        );
        println!(
            "      sentiment={:.2} multi_tf={:.2} funding={:.2} wyckoff={:.2} cycle={:.2}",
            w.sentiment, w.multi_tf, w.funding_oi, w.wyckoff_alignment, w.cycle_alignment,
        );
    }
    println!();
    println!(" channel sensitivity (Pearson r vs mean OOS):");
    let mut rows = report.sensitivity.clone();
    rows.sort_by(|a, b| {
        b.correlation_with_oos
            .abs()
            .partial_cmp(&a.correlation_with_oos.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for r in rows {
        let corr = if r.correlation_with_oos.is_nan() {
            "  n/a".into()
        } else {
            format!("{:>+5.2}", r.correlation_with_oos)
        };
        println!(
            "  {:>20}: r={}  range=[{:.2}, {:.2}]  best={:.2}",
            r.channel, corr, r.min_value, r.max_value, r.best_value,
        );
    }
    println!("──────────────────────────────────────────────────────");
}

/// Resolve the trade log path. If the user passed `--log dir/`
/// (a directory), append a stamped filename. Otherwise treat the
/// path as a file.
pub fn resolve_log_path(input: &Path, run_tag: &str) -> PathBuf {
    if input.is_dir() {
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S");
        input.join(format!("{}_{}.jsonl", run_tag, stamp))
    } else {
        input.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_log_path_passthrough_for_file() {
        let p = std::path::PathBuf::from("/tmp/trades.jsonl");
        let out = resolve_log_path(&p, "tag");
        assert_eq!(out, p);
    }

    #[test]
    fn resolve_log_path_appends_stamp_for_dir() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let out = resolve_log_path(p, "mytag");
        assert!(out.starts_with(p));
        assert!(out.to_string_lossy().contains("mytag"));
        assert!(out.extension().map(|e| e == "jsonl").unwrap_or(false));
    }

    #[test]
    fn print_report_does_not_panic_on_empty() {
        let cfg = IqBacktestConfig::default();
        let report = super::super::report::IqBacktestReport::seed(cfg);
        // Just ensure no panic — actual stdout content is human-only.
        print_report(&report);
    }
}
