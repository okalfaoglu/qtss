//! Per-trade JSON-line writer.
//!
//! Each closed trade gets one line: `{trade, attribution}`. Plays
//! nicely with `jq`, DuckDB, pandas — analyst can pivot on
//! `attribution.class` / `attribution.loss_reason` without
//! re-running the backtest.
//!
//! Optional — runner instantiates only when
//! `IqBacktestConfig.trade_log_path = Some(_)`.

use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use super::attribution::OutcomeAttribution;
use super::trade::IqTrade;

#[derive(Debug, Serialize)]
struct TradeLogRow<'a> {
    trade: &'a IqTrade,
    attribution: &'a OutcomeAttribution,
}

/// Append-only JSONL writer. Buffered + flushed per write so a
/// crash mid-run doesn't lose the trades that ran before it. Mutex
/// because the runner could one day fan out symbols across tasks.
pub struct TradeLogWriter {
    inner: Mutex<BufWriter<File>>,
    path: PathBuf,
    rows_written: std::sync::atomic::AtomicU64,
}

impl TradeLogWriter {
    /// Open or create the JSONL file. If the file already exists,
    /// new lines append (we never truncate — backtest runs are
    /// additive within a `run_tag`).
    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            inner: Mutex::new(BufWriter::new(file)),
            path,
            rows_written: std::sync::atomic::AtomicU64::new(0),
        })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn rows_written(&self) -> u64 {
        self.rows_written
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Append one row. Returns the new row count. Errors propagate
    /// — backtests should fail loud if disk fills up rather than
    /// silently dropping trades.
    pub fn write_row(
        &self,
        trade: &IqTrade,
        attribution: &OutcomeAttribution,
    ) -> std::io::Result<u64> {
        let row = TradeLogRow { trade, attribution };
        let line = serde_json::to_string(&row).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;
        let mut guard = self.inner.lock().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "trade-log mutex poisoned",
            )
        })?;
        guard.write_all(line.as_bytes())?;
        guard.write_all(b"\n")?;
        guard.flush()?;
        let n = self
            .rows_written
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iq::attribution::classify;
    use crate::iq::config::IqPolarity;
    use crate::iq::trade::{IqTrade, TradeOutcome, TradeState};
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use serde_json::json;
    use std::io::BufRead;

    #[test]
    fn writer_appends_one_jsonline_per_trade() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trades.jsonl");
        let writer = TradeLogWriter::open(path.clone()).unwrap();

        let mut t = IqTrade::pending(
            "test",
            IqPolarity::Dip,
            "BTCUSDT",
            "4h",
            "binance",
            "futures",
            100,
            Utc::now(),
            dec!(50000),
            dec!(48000),
            vec![dec!(52000)],
            dec!(0.1),
            json!({"structural_completion": 0.7}),
            0.7,
        );
        t.state = TradeState::Closed;
        t.outcome = Some(TradeOutcome::StopLoss);
        t.net_pnl_pct = -1.0;
        t.max_adverse_pct = -1.5;
        let attr = classify(&t);

        writer.write_row(&t, &attr).unwrap();
        writer.write_row(&t, &attr).unwrap();
        assert_eq!(writer.rows_written(), 2);

        // Re-open and read back to confirm both lines wrote.
        let f = std::fs::File::open(&path).unwrap();
        let lines: Vec<_> =
            std::io::BufReader::new(f).lines().collect::<Result<_, _>>().unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"trade_id\""));
        assert!(lines[0].contains("\"attribution\""));
    }
}
