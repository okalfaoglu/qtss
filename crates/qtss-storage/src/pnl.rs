//! Canlı / dry ledger için özet P&L rollup (dashboard ve raporlama).
//!
//! `rebuild_live_rollups_from_exchange_orders`: `exchange_orders.venue_response` içindeki Binance
//! yanıtlarından (`executedQty` / `cummulativeQuoteQty`) günlük/haftalık/aylık/yıllık hacim ve
//! ücret özeti üretir. Basit realized P&L hesaplaması (average cost) ile `realized_pnl` ve
//! pozisyon kapandı sayımı ile `closed_trade_count` üretir.
//!
//! ## Mimari not
//! Binance’a özgü JSON ayrıştırması şu an bu dosyada (venue yanıtı şeması). Yeni borsa eklenirken
//! ayrıştırma `qtss-binance` / venue adapter katmanına taşınıp buraya normalize edilmiş fill
//! struct’ları iletilmeli (soyutlama sızıntısını gidermek için).
//!
//! `rebuild_live_rollups_from_exchange_orders` tüm `live` rollup satırlarını siler ve baştan üretir;
//! büyük tablolarda kilit süresi riski vardır — ileride artımlı (incremental) yeniden hesap tercih edilmeli.

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
    pub segment: String,
    pub symbol: Option<String>,
    pub ledger: String,
    pub bucket: String,
    pub period_start: DateTime<Utc>,
    pub realized_pnl: Decimal,
    pub fees: Decimal,
    pub volume: Decimal,
    pub trade_count: i64,
    pub closed_trade_count: i64,
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
    segment: String,
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
    closed_trade_count: i64,
}

#[derive(Default, Clone)]
struct PositionAcc {
    qty: Decimal,
    avg_cost_quote_per_base: Decimal,
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
#[derive(Debug, Clone)]
struct BinanceFill {
    qty: Decimal,
    price: Decimal,
    commission: Decimal,
    commission_asset: Option<String>,
}

fn dec_from_str_field(v: &serde_json::Value, k: &str) -> Option<Decimal> {
    let s = v.get(k)?.as_str()?;
    Decimal::from_str(s.trim()).ok()
}

fn parse_binance_fills(v: &serde_json::Value) -> Vec<BinanceFill> {
    let mut out = Vec::new();
    let Some(arr) = v.get("fills").and_then(|x| x.as_array()) else {
        return out;
    };
    for f in arr {
        let Some(qty) = dec_from_str_field(f, "qty") else {
            continue;
        };
        let Some(price) = dec_from_str_field(f, "price") else {
            continue;
        };
        let commission = dec_from_str_field(f, "commission").unwrap_or(Decimal::ZERO);
        let commission_asset = f
            .get("commissionAsset")
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if qty > Decimal::ZERO && price > Decimal::ZERO {
            out.push(BinanceFill {
                qty,
                price,
                commission,
                commission_asset,
            });
        }
    }
    out
}

#[derive(Debug, Clone)]
struct InstrumentAssets {
    base_asset: String,
    quote_asset: String,
}

async fn fetch_instrument_assets(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    native_symbol: &str,
) -> Result<Option<InstrumentAssets>, StorageError> {
    let row = sqlx::query_as::<_, (String, String)>(
        r#"SELECT i.base_asset, i.quote_asset
           FROM instruments i
           INNER JOIN markets m ON m.id = i.market_id
           INNER JOIN exchanges e ON e.id = m.exchange_id
           WHERE e.code = $1 AND m.segment = $2 AND UPPER(TRIM(i.native_symbol)) = UPPER(TRIM($3))
           LIMIT 1"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(native_symbol)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(base_asset, quote_asset)| InstrumentAssets { base_asset, quote_asset }))
}

fn price_lookup_bucket_10m(at: DateTime<Utc>) -> i64 {
    at.timestamp() / 600
}

type AssetQuotePriceCacheKey = (String, String, String, String, i64);

async fn fetch_asset_price_in_quote(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    asset: &str,
    quote_asset: &str,
    at: DateTime<Utc>,
) -> Result<Option<Decimal>, StorageError> {
    // Try direct symbol: e.g. BNBUSDT for asset=BNB quote=USDT.
    let sym = format!("{}{}", asset.trim().to_uppercase(), quote_asset.trim().to_uppercase());
    let start = at - Duration::minutes(10);
    let end = at + Duration::minutes(10);
    let row = sqlx::query_scalar::<_, Decimal>(
        r#"SELECT close
           FROM market_bars
           WHERE exchange = $1 AND segment = $2 AND symbol = $3 AND interval = '1m'
             AND open_time >= $4 AND open_time <= $5
           ORDER BY ABS(EXTRACT(EPOCH FROM (open_time - $6))) ASC
           LIMIT 1"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(&sym)
    .bind(start)
    .bind(end)
    .bind(at)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

async fn normalize_commission_to_quote(
    pool: &PgPool,
    price_cache: &mut HashMap<AssetQuotePriceCacheKey, Option<Decimal>>,
    exchange: &str,
    segment: &str,
    base_asset: &str,
    quote_asset: &str,
    fill_price_quote_per_base: Decimal,
    commission: Decimal,
    commission_asset: Option<&str>,
    at: DateTime<Utc>,
) -> Result<Decimal, StorageError> {
    if commission <= Decimal::ZERO {
        return Ok(Decimal::ZERO);
    }
    let Some(ca) = commission_asset.map(|s| s.trim()).filter(|s| !s.is_empty()) else {
        // Unknown unit; leave as-is (best effort assumes quote).
        return Ok(commission);
    };
    if ca.eq_ignore_ascii_case(quote_asset) {
        return Ok(commission);
    }
    if ca.eq_ignore_ascii_case(base_asset) {
        return Ok(commission * fill_price_quote_per_base);
    }
    let bucket = price_lookup_bucket_10m(at);
    let cache_key = (
        exchange.to_string(),
        segment.to_string(),
        ca.to_uppercase(),
        quote_asset.to_uppercase(),
        bucket,
    );
    let px = if let Some(hit) = price_cache.get(&cache_key) {
        *hit
    } else {
        let fetched = fetch_asset_price_in_quote(pool, exchange, segment, ca, quote_asset, at).await?;
        price_cache.insert(cache_key.clone(), fetched);
        fetched
    };
    let Some(px) = px else {
        tracing::warn!(
            exchange = %exchange,
            segment = %segment,
            commission_asset = %ca,
            quote_asset = %quote_asset,
            "pnl: cannot convert commission asset to quote; fee treated as zero (realized PnL may be overstated)"
        );
        return Ok(Decimal::ZERO);
    };
    Ok(commission * px)
}

fn position_side_key(v: &serde_json::Value) -> &'static str {
    let raw = v
        .get("positionSide")
        .and_then(|x| x.as_str())
        .unwrap_or("BOTH");
    if raw.eq_ignore_ascii_case("LONG") {
        "long"
    } else if raw.eq_ignore_ascii_case("SHORT") {
        "short"
    } else {
        "both"
    }
}

fn signed_qty_delta(side: &str, position_side: &str, qty: Decimal) -> Decimal {
    let is_buy = side.eq_ignore_ascii_case("BUY");
    let is_sell = side.eq_ignore_ascii_case("SELL");
    if position_side == "long" {
        if is_buy {
            qty
        } else if is_sell {
            -qty
        } else {
            Decimal::ZERO
        }
    } else if position_side == "short" {
        if is_sell {
            -qty
        } else if is_buy {
            qty
        } else {
            Decimal::ZERO
        }
    } else {
        if is_buy {
            qty
        } else if is_sell {
            -qty
        } else {
            Decimal::ZERO
        }
    }
}

fn apply_fill_to_position(p: &mut PositionAcc, delta_qty: Decimal, fill_price: Decimal) -> (Decimal, bool) {
    if delta_qty == Decimal::ZERO || fill_price <= Decimal::ZERO {
        return (Decimal::ZERO, false);
    }

    let qty_before = p.qty;
    if qty_before == Decimal::ZERO {
        p.qty = delta_qty;
        p.avg_cost_quote_per_base = fill_price;
        return (Decimal::ZERO, false);
    }

    let same_dir = (qty_before > Decimal::ZERO && delta_qty > Decimal::ZERO)
        || (qty_before < Decimal::ZERO && delta_qty < Decimal::ZERO);
    if same_dir {
        let abs_before = qty_before.abs();
        let abs_delta = delta_qty.abs();
        let abs_after = abs_before + abs_delta;
        let cost_before = p.avg_cost_quote_per_base * abs_before;
        let cost_delta = fill_price * abs_delta;
        p.avg_cost_quote_per_base = (cost_before + cost_delta) / abs_after;
        p.qty = qty_before + delta_qty;
        return (Decimal::ZERO, false);
    }

    let abs_before = qty_before.abs();
    let abs_delta = delta_qty.abs();
    let closed_abs = abs_before.min(abs_delta);
    let mut realized = Decimal::ZERO;
    if qty_before > Decimal::ZERO {
        realized = (fill_price - p.avg_cost_quote_per_base) * closed_abs;
    } else if qty_before < Decimal::ZERO {
        realized = (p.avg_cost_quote_per_base - fill_price) * closed_abs;
    }

    let remaining_delta_abs = abs_delta - closed_abs;
    let next_qty = qty_before + delta_qty;

    let closed_trade = abs_before > Decimal::ZERO && next_qty == Decimal::ZERO;

    if next_qty == Decimal::ZERO {
        p.qty = Decimal::ZERO;
        p.avg_cost_quote_per_base = Decimal::ZERO;
        return (realized, closed_trade);
    }

    if remaining_delta_abs > Decimal::ZERO {
        p.qty = if delta_qty > Decimal::ZERO {
            remaining_delta_abs
        } else {
            -remaining_delta_abs
        };
        p.avg_cost_quote_per_base = fill_price;
    } else {
        p.qty = next_qty;
    }

    (realized, closed_trade)
}

#[derive(Debug, Clone)]
struct ParsedBinanceOrder {
    executed_qty: Decimal,
    cumm_quote: Decimal,
    side: String,
    position_side: &'static str,
    fills: Vec<BinanceFill>,
}

fn parse_binance_order(v: &serde_json::Value) -> Option<ParsedBinanceOrder> {
    let status = v.get("status")?.as_str()?;
    let ok = matches!(status, "FILLED" | "PARTIALLY_FILLED");
    if !ok {
        return None;
    }
    let executed_qty = Decimal::from_str(v.get("executedQty")?.as_str().unwrap_or("0")).ok()?;
    if executed_qty <= Decimal::ZERO {
        return None;
    }
    let cumm_quote = Decimal::from_str(v.get("cummulativeQuoteQty")?.as_str().unwrap_or("0")).ok()?;
    let side = v.get("side").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let position_side = position_side_key(v);
    let fills = parse_binance_fills(v);
    Some(ParsedBinanceOrder {
        executed_qty,
        cumm_quote,
        side,
        position_side,
        fills,
    })
}

impl PnlRollupRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Dashboard: tek organizasyon; `ledger` + `bucket` (`daily` / `weekly` / …) + optional instrument filters.
    pub async fn list_rollups(
        &self,
        org_id: Uuid,
        ledger: &str,
        bucket: &str,
        exchange: Option<&str>,
        segment: Option<&str>,
        symbol: Option<&str>,
        limit: i64,
    ) -> Result<Vec<PnlRollupRow>, StorageError> {
        let rows = sqlx::query_as::<_, PnlRollupRow>(
            r#"SELECT org_id, exchange, segment, symbol, ledger, bucket, period_start,
                      realized_pnl, fees, volume, trade_count, closed_trade_count
               FROM pnl_rollups
               WHERE org_id = $1 AND ledger = $2 AND bucket = $3
                 AND ($4::text IS NULL OR exchange = $4)
                 AND ($5::text IS NULL OR segment = $5)
                 AND ($6::text IS NULL OR symbol = $6)
               ORDER BY period_start DESC
               LIMIT $7"#,
        )
        .bind(org_id)
        .bind(ledger)
        .bind(bucket)
        .bind(exchange)
        .bind(segment)
        .bind(symbol)
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

        tracing::info!("pnl: rebuilding all live rollups (full DELETE + recompute; may lock pnl_rollups briefly)");
        sqlx::query(r#"DELETE FROM pnl_rollups WHERE ledger = 'live'"#)
            .execute(&mut *tx)
            .await?;

        #[derive(Debug, sqlx::FromRow)]
        struct OrderRow {
            org_id: Uuid,
            exchange: String,
            segment: String,
            symbol: String,
            updated_at: DateTime<Utc>,
            #[sqlx(json)]
            venue_response: serde_json::Value,
        }

        let orders: Vec<OrderRow> = sqlx::query_as(
            r#"SELECT org_id, exchange, segment, symbol, updated_at, venue_response
               FROM exchange_orders
               WHERE venue_response IS NOT NULL
               ORDER BY updated_at ASC"#,
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut orders_with_fills = 0usize;
        let mut acc: HashMap<RollupAggKey, RollupAcc> = HashMap::new();
        let mut pos: HashMap<(Uuid, String, String, &'static str), PositionAcc> = HashMap::new();
        let mut instrument_cache: HashMap<(String, String, String), InstrumentAssets> = HashMap::new();
        let mut commission_price_cache: HashMap<AssetQuotePriceCacheKey, Option<Decimal>> = HashMap::new();

        for o in &orders {
            let vr = &o.venue_response;
            let Some(parsed) = parse_binance_order(vr) else {
                continue;
            };
            orders_with_fills += 1;

            let mut volume_quote = Decimal::ZERO;
            let mut fees = Decimal::ZERO;
            let mut realized_pnl = Decimal::ZERO;
            let mut closed_trade_count = 0i64;
            let mut trade_count = 0i64;

            let cache_key = (o.exchange.clone(), o.segment.clone(), o.symbol.clone());
            let inst = if let Some(x) = instrument_cache.get(&cache_key) {
                x.clone()
            } else {
                let fetched = fetch_instrument_assets(&self.pool, &o.exchange, &o.segment, &o.symbol).await?;
                let Some(fetched) = fetched else {
                    continue;
                };
                instrument_cache.insert(cache_key.clone(), fetched.clone());
                fetched
            };

            let key_pos = (o.org_id, o.exchange.clone(), o.symbol.clone(), parsed.position_side);
            let p = pos.entry(key_pos).or_default();

            if !parsed.fills.is_empty() {
                for f in &parsed.fills {
                    fees += normalize_commission_to_quote(
                        &self.pool,
                        &mut commission_price_cache,
                        &o.exchange,
                        &o.segment,
                        &inst.base_asset,
                        &inst.quote_asset,
                        f.price,
                        f.commission,
                        f.commission_asset.as_deref(),
                        o.updated_at,
                    )
                    .await?;
                    volume_quote += f.qty * f.price;
                    let dqty = signed_qty_delta(&parsed.side, parsed.position_side, f.qty);
                    let (rp, closed) = apply_fill_to_position(p, dqty, f.price);
                    realized_pnl += rp;
                    if closed {
                        closed_trade_count += 1;
                    }
                    trade_count += 1;
                }
            } else {
                // Aggregate fallback: treat it as a single fill at average price.
                let avg_px = if parsed.executed_qty > Decimal::ZERO {
                    parsed.cumm_quote / parsed.executed_qty
                } else {
                    Decimal::ZERO
                };
                if avg_px > Decimal::ZERO {
                    volume_quote = parsed.cumm_quote;
                    let dqty = signed_qty_delta(&parsed.side, parsed.position_side, parsed.executed_qty);
                    let (rp, closed) = apply_fill_to_position(p, dqty, avg_px);
                    realized_pnl += rp;
                    if closed {
                        closed_trade_count += 1;
                    }
                    trade_count = 1;
                }
            }

            for (bucket, period_start) in period_starts(o.updated_at) {
                let key = RollupAggKey {
                    org_id: o.org_id,
                    exchange: o.exchange.clone(),
                    segment: o.segment.clone(),
                    symbol: o.symbol.clone(),
                    bucket,
                    period_start,
                };
                let e = acc.entry(key).or_default();
                e.volume += volume_quote;
                e.fees += fees;
                e.trade_count += trade_count;
                e.realized_pnl += realized_pnl;
                e.closed_trade_count += closed_trade_count;
            }
        }

        let mut written = 0usize;
        for (k, v) in acc {
            sqlx::query(
                r#"INSERT INTO pnl_rollups (
                       org_id, exchange, segment, symbol, ledger, bucket, period_start,
                       realized_pnl, fees, volume, trade_count, closed_trade_count
                   ) VALUES ($1, $2, $3, $4, 'live', $5, $6, $7, $8, $9, $10, $11)"#,
            )
            .bind(k.org_id)
            .bind(&k.exchange)
            .bind(&k.segment)
            .bind(&k.symbol)
            .bind(k.bucket)
            .bind(k.period_start)
            .bind(v.realized_pnl)
            .bind(v.fees)
            .bind(v.volume)
            .bind(v.trade_count)
            .bind(v.closed_trade_count)
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

/// Kill-switch / risk: sum `realized_pnl` across all rows for the current UTC `daily` bucket start.
pub async fn sum_today_daily_realized_pnl(pool: &PgPool) -> Result<Decimal, StorageError> {
    let d = Utc::now().date_naive();
    let t0 = utc_day_start(d);
    let sum: Option<Decimal> = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(realized_pnl), 0)
           FROM pnl_rollups
           WHERE bucket = 'daily' AND period_start = $1"#,
    )
    .bind(t0)
    .fetch_one(pool)
    .await?;
    Ok(sum.unwrap_or(Decimal::ZERO))
}
