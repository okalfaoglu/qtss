//! Canlı / dry ledger için özet P&L rollup (dashboard ve raporlama).
//!
//! `rebuild_live_rollups_from_exchange_orders`: `exchange_orders.venue_response` içindeki Binance
//! yanıtlarından (`executedQty` / `cummulativeQuoteQty`) günlük/haftalık/aylık/yıllık hacim ve
//! ücret özeti üretir. **Gerçekleşen P&L** pozisyon motoru olmadan hesaplanamaz; şimdilik `0`.

use std::collections::HashMap;
use std::str::FromStr;

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PnlBucket {
    Instant,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct PnlRollupRow {
    pub org_id: Uuid,
    pub exchange: String,
    pub symbol: Option<String>,
    pub ledger: String,
    pub bucket: String,
    pub period_start: DateTime<Utc>,
    pub realized_pnl: Decimal,
    pub fees: Decimal,
    pub volume: Decimal,
    pub trade_count: i64,
}

pub struct PnlRollupRepository {
    pool: PgPool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PnlRebuildStats {
    pub orders_scanned: usize,
    pub orders_with_fills: usize,
    pub rollup_rows_written: usize,
}

#[derive(Eq, PartialEq, Hash, Clone)]
struct RollupAggKey {
    org_id: Uuid,
    exchange: String,
    symbol: String,
    bucket: &'static str,
    period_start: DateTime<Utc>,
}

#[derive(Default, Clone)]
struct RollupAcc {
    realized_pnl: Decimal,
    fees: Decimal,
    volume: Decimal,
    trade_count: i64,
}

fn utc_day_start(d: NaiveDate) -> DateTime<Utc> {
    let nt = d
        .and_hms_opt(0, 0, 0)
        .expect("midnight valid for calendar date");
    Utc.from_utc_datetime(&nt)
}

fn period_starts(ts: DateTime<Utc>) -> [(&'static str, DateTime<Utc>); 4] {
    let d = ts.date_naive();
    let daily = utc_day_start(d);
    let wd = d.weekday().num_days_from_monday();
    let week_naive = d - Duration::days(i64::from(wd));
    let weekly = utc_day_start(week_naive);
    let month_naive =
        NaiveDate::from_ymd_opt(d.year(), d.month(), 1).expect("month day 1");
    let monthly = utc_day_start(month_naive);
    let year_naive = NaiveDate::from_ymd_opt(d.year(), 1, 1).expect("jan 1");
    let yearly = utc_day_start(year_naive);
    [
        ("daily", daily),
        ("weekly", weekly),
        ("monthly", monthly),
        ("yearly", yearly),
    ]
}

/// Binance yeni emir / sorgu yanıtı: en azından `executedQty` ve `cummulativeQuoteQty`.
fn binance_order_trade_contribution(v: &serde_json::Value) -> Option<(Decimal, Decimal)> {
    let status = v.get("status")?.as_str()?;
    let ok = matches!(status, "FILLED" | "PARTIALLY_FILLED");
    let ex = Decimal::from_str(v.get("executedQty")?.as_str().unwrap_or("0")).ok()?;
    if ex <= Decimal::ZERO {
        return None;
    }
    if !ok {
        return None;
    }
    let quote = Decimal::from_str(v.get("cummulativeQuoteQty")?.as_str().unwrap_or("0")).ok()?;
    let mut fee = Decimal::ZERO;
    if let Some(fills) = v.get("fills").and_then(|x| x.as_array()) {
        for f in fills {
            if let Some(c) = f
                .get("commission")
                .and_then(|x| x.as_str())
                .and_then(|s| Decimal::from_str(s).ok())
            {
                fee += c;
            }
        }
    }
    Some((quote, fee))
}

impl PnlRollupRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Dashboard: tek organizasyon; `ledger` + `bucket` (`daily` / `weekly` / …).
    pub async fn list_rollups(
        &self,
        org_id: Uuid,
        ledger: &str,
        bucket: &str,
        limit: i64,
    ) -> Result<Vec<PnlRollupRow>, StorageError> {
        let rows = sqlx::query_as::<_, PnlRollupRow>(
            r#"SELECT org_id, exchange, symbol, ledger, bucket, period_start,
                      realized_pnl, fees, volume, trade_count
               FROM pnl_rollups
               WHERE org_id = $1 AND ledger = $2 AND bucket = $3
               ORDER BY period_start DESC
               LIMIT $4"#,
        )
        .bind(org_id)
        .bind(ledger)
        .bind(bucket)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// `ledger = live` rollup’ları `exchange_orders` üzerinden baştan üretir (idempotent).
    pub async fn rebuild_live_rollups_from_exchange_orders(
        &self,
    ) -> Result<PnlRebuildStats, StorageError> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(r#"DELETE FROM pnl_rollups WHERE ledger = 'live'"#)
            .execute(&mut *tx)
            .await?;

        #[derive(Debug, sqlx::FromRow)]
        struct OrderRow {
            org_id: Uuid,
            exchange: String,
            symbol: String,
            updated_at: DateTime<Utc>,
            #[sqlx(json)]
            venue_response: serde_json::Value,
        }

        let orders: Vec<OrderRow> = sqlx::query_as(
            r#"SELECT org_id, exchange, symbol, updated_at, venue_response
               FROM exchange_orders
               WHERE venue_response IS NOT NULL
               ORDER BY updated_at ASC"#,
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut orders_with_fills = 0usize;
        let mut acc: HashMap<RollupAggKey, RollupAcc> = HashMap::new();

        for o in &orders {
            let vr = &o.venue_response;
            let Some((quote, fee)) = binance_order_trade_contribution(vr) else {
                continue;
            };
            orders_with_fills += 1;
            for (bucket, period_start) in period_starts(o.updated_at) {
                let key = RollupAggKey {
                    org_id: o.org_id,
                    exchange: o.exchange.clone(),
                    symbol: o.symbol.clone(),
                    bucket,
                    period_start,
                };
                let e = acc.entry(key).or_default();
                e.volume += quote;
                e.fees += fee;
                e.trade_count += 1;
            }
        }

        let mut written = 0usize;
        for (k, v) in acc {
            sqlx::query(
                r#"INSERT INTO pnl_rollups (
                       org_id, exchange, symbol, ledger, bucket, period_start,
                       realized_pnl, fees, volume, trade_count
                   ) VALUES ($1, $2, $3, 'live', $4, $5, $6, $7, $8, $9)"#,
            )
            .bind(k.org_id)
            .bind(&k.exchange)
            .bind(&k.symbol)
            .bind(k.bucket)
            .bind(k.period_start)
            .bind(v.realized_pnl)
            .bind(v.fees)
            .bind(v.volume)
            .bind(v.trade_count)
            .execute(&mut *tx)
            .await?;
            written += 1;
        }

        tx.commit().await?;

        Ok(PnlRebuildStats {
            orders_scanned: orders.len(),
            orders_with_fills,
            rollup_rows_written: written,
        })
    }
}
