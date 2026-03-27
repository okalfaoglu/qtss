//! PDF rapor üretimi (özet P&L, backtest metrikleri). İleride şablon motoru eklenebilir.

mod pdf;

pub use pdf::{PdfError, ReportRenderer};
