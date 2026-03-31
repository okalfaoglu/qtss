//! Central logging on `tracing` (QTSS policy: `docs/QTSS_MASTER_DEV_GUIDE.md` §8.1).
//!
//! ## Levels (which macro / helper)
//!
//! | Level    | Use |
//! |----------|-----|
//! | `trace`  | Extremely verbose flow (off in default production filters). `tracing::trace!` |
//! | `debug`  | Developer diagnostics, per-tick / per-symbol detail. `tracing::debug!` |
//! | `info`   | Normal lifecycle: startup, successful loop iteration summaries. `tracing::info!` |
//! | `warn`   | Recoverable anomalies: retry, missing optional data, transient upstream errors. `tracing::warn!` |
//! | `error`  | Serious failure: persistence broken, invariant violated, non-recoverable step. `tracing::error!` |
//! | `critical` | Operator action required; emitted as `error!` with `is_critical = true`. [`log_critical`] or [`log_business`](QtssLogLevel::Critical) |
//!
//! Prefer [`log_business`] when routing through the `qtss` target with a stable `qtss_module` label.
//! Filter with `RUST_LOG` (e.g. `qtss=debug`, `qtss_api=info`, `qtss_worker=info`).

use serde::Serialize;
use std::borrow::Cow;
use tracing::Level;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// İş domain’inde kullanılan etiket seviyesi (filtreleme ve raporlama için).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QtssLogLevel {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

impl QtssLogLevel {
    pub fn from_tracing_level(level: Level) -> Self {
        match level {
            Level::ERROR => Self::Error,
            Level::WARN => Self::Warning,
            Level::INFO => Self::Info,
            Level::DEBUG | Level::TRACE => Self::Debug,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEvent<'a> {
    pub level: QtssLogLevel,
    pub module: Cow<'a, str>,
    pub message: Cow<'a, str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_critical: Option<bool>,
}

/// Mantıksal modül etiketi (`qtss_module` alanı); tracing `target`ı sabit `qtss`.
pub trait Loggable {
    const MODULE: &'static str;
}

/// Varsayılan subscriber: düz metin satırlar.
pub fn init_logging(default_directive: &str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_directive));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true).with_line_number(true))
        .init();
}

/// Kritik olayı `error` seviyesinde işaretle (alerting buraya bağlanabilir).
pub fn log_critical(module: &str, message: impl AsRef<str>) {
    tracing::error!(
        target: "qtss",
        is_critical = true,
        qtss_module = %module,
        "{}",
        message.as_ref()
    );
}

/// Seviye ile düzgün `tracing` event.
pub fn log_business(level: QtssLogLevel, module: &str, message: impl AsRef<str>) {
    let msg = message.as_ref();
    match level {
        QtssLogLevel::Debug => {
            tracing::debug!(target: "qtss", qtss_module = %module, "{}", msg);
        }
        QtssLogLevel::Info => {
            tracing::info!(target: "qtss", qtss_module = %module, "{}", msg);
        }
        QtssLogLevel::Warning => {
            tracing::warn!(target: "qtss", qtss_module = %module, "{}", msg);
        }
        QtssLogLevel::Error => {
            tracing::error!(target: "qtss", qtss_module = %module, "{}", msg);
        }
        QtssLogLevel::Critical => {
            tracing::error!(
                target: "qtss",
                is_critical = true,
                qtss_module = %module,
                "{}",
                msg
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_levels_order() {
        assert!(QtssLogLevel::Debug < QtssLogLevel::Critical);
    }
}
