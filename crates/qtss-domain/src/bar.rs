use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// İşlem/analiz için birincil veri birimi: **tick değil**, zaman damgalı bar.
/// Tick altyapısı ileride ayrı `TickStream` trait’i ile eklenecek.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampBar {
    pub ts: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
}

pub trait TimestampBarFeed: Send {
    fn next_bar(&mut self) -> Option<TimestampBar>;
}
