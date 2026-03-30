//! Copy-trade: `nansen_perp_trades` yön skoru + `CopyRule` → [`ExecutionGateway::place`] (dev guide ADIM 7, §3.4).
//!
//! Lider dolum kuyruğu yok; smart-money perp aggregate yönü takipçi hesabına paper/canlı market emri olarak yansıtılır.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use qtss_common::is_trading_halted;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{OrderIntent, OrderSide, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use qtss_domain::CopyRule;
use qtss_execution::ExecutionGateway;
use qtss_storage::{data_snapshot_age_secs, fetch_data_snapshot, list_recent_bars};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::risk::{apply_kelly_scale_to_qty, clamp_qty_by_max_notional_usdt};

const NANSEN_PERP_TRADES_KEY: &str = "nansen_perp_trades";

fn tick_secs() -> u64 {
    std::env::var("QTSS_COPY_TRADE_STRATEGY_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120)
        .max(30)
}

fn direction_threshold() -> f64 {
    std::env::var("QTSS_COPY_TRADE_DIRECTION_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.25)
}

fn base_order_qty() -> Decimal {
    let base = std::env::var("QTSS_COPY_TRADE_BASE_QTY")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .or_else(|| {
            std::env::var("QTSS_STRATEGY_ORDER_QTY")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or_else(|| Decimal::new(1, 3));
    apply_kelly_scale_to_qty(base)
}

fn default_symbol() -> String {
    std::env::var("QTSS_COPY_TRADE_DEFAULT_SYMBOL")
        .unwrap_or_else(|_| "BTCUSDT".into())
        .trim()
        .to_uppercase()
}

fn bar_ctx() -> (String, String, String) {
    let ex = std::env::var("QTSS_COPY_TRADE_BAR_EXCHANGE").unwrap_or_else(|_| "binance".into());
    let seg = std::env::var("QTSS_COPY_TRADE_BAR_SEGMENT").unwrap_or_else(|_| "futures".into());
    let iv = std::env::var("QTSS_COPY_TRADE_BAR_INTERVAL").unwrap_or_else(|_| "1m".into());
    (ex, seg, iv)
}

fn auto_place() -> bool {
    std::env::var("QTSS_COPY_TRADE_STRATEGY_AUTO_PLACE")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn requires_human_approval() -> bool {
    !std::env::var("QTSS_STRATEGY_SKIP_HUMAN_APPROVAL")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

/// Worker `score_nansen_perp_direction` ile aynı mantık (crate ayrımı için kopya).
#[must_use]
fn perp_long_ratio_score(response: &Value) -> f64 {
    let Some(rows) = response.get("data").and_then(|d| d.as_array()) else {
        return 0.0;
    };
    let mut long_n = 0_f64;
    let mut short_n = 0_f64;
    for row in rows.iter().take(2000) {
        let side = row
            .get("side")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let n = row
            .get("notional_usd")
            .and_then(|x| x.as_f64())
            .or_else(|| row.get("notionalUsd").and_then(|x| x.as_f64()))
            .unwrap_or(0.0)
            .max(0.0);
        if side.contains("long") {
            long_n += n;
        } else if side.contains("short") {
            short_n += n;
        }
    }
    let t = long_n + short_n;
    if t < 1e-12 {
        return 0.0;
    }
    let ratio = long_n / t;
    ((ratio - 0.5) * 2.0).clamp(-1.0, 1.0)
}

async fn perp_data_fresh_ms(pool: &PgPool, max_latency_ms: i64) -> bool {
    if max_latency_ms <= 0 {
        return true;
    }
    match data_snapshot_age_secs(pool, NANSEN_PERP_TRADES_KEY).await {
        Ok(Some(secs)) if secs >= 0 => {
            let ms = secs.saturating_mul(1000);
            if ms > max_latency_ms {
                warn!(
                    age_ms = ms,
                    max_ms = max_latency_ms,
                    "copy_trade: perp snapshot bayat — emir yok"
                );
                false
            } else {
                true
            }
        }
        Ok(None) => {
            warn!("copy_trade: nansen_perp_trades snapshot yok — emir yok");
            false
        }
        Ok(Some(_)) => false,
        Err(e) => {
            warn!(%e, "copy_trade: data_snapshot_age_secs");
            false
        }
    }
}

fn pick_symbol(rule: &CopyRule) -> String {
    if let Some(s) = rule
        .symbol_allowlist
        .iter()
        .map(|x| x.trim().to_uppercase())
        .find(|x| !x.is_empty())
    {
        s
    } else {
        default_symbol()
    }
}

fn apply_notional_bounds(rule: &CopyRule, qty: Decimal, mark: Decimal) -> Option<Decimal> {
    let notional = qty * mark;
    if let Some(mn) = rule.min_notional {
        if notional < mn {
            warn!(%notional, min = %mn, "copy_trade: min_notional altında");
            return None;
        }
    }
    let mut q = qty;
    if let Some(mx) = rule.max_notional {
        if q * mark > mx && mark > Decimal::ZERO {
            q = mx / mark;
            if q <= Decimal::ZERO {
                return None;
            }
        }
    }
    Some(q)
}

pub async fn run(pool: PgPool, gateway: Arc<dyn ExecutionGateway>) {
    let tick = Duration::from_secs(tick_secs());
    let repo = qtss_storage::CopySubscriptionRepository::new(pool.clone());
    let (bar_ex, bar_seg, bar_iv) = bar_ctx();
    info!(
        poll_secs = tick.as_secs(),
        "copy_trade strateji döngüsü (perp aggregate)"
    );
    loop {
        tokio::time::sleep(tick).await;
        if is_trading_halted() {
            continue;
        }
        let snap_row = match fetch_data_snapshot(&pool, NANSEN_PERP_TRADES_KEY).await {
            Ok(r) => r,
            Err(e) => {
                warn!(%e, "copy_trade fetch_data_snapshot");
                continue;
            }
        };
        let Some(j) = snap_row.and_then(|r| r.response_json) else {
            tracing::debug!("copy_trade: perp JSON yok");
            continue;
        };
        let score = perp_long_ratio_score(&j);
        let th = direction_threshold();

        match repo.list_active_subscriptions().await {
            Ok(rows) => {
                for r in rows {
                    let rule: Result<CopyRule, _> = serde_json::from_value(r.rule.clone());
                    let Ok(rule) = rule else {
                        warn!(sub_id = %r.id, "copy_trade: CopyRule parse");
                        continue;
                    };
                    if !perp_data_fresh_ms(&pool, rule.max_latency_ms).await {
                        continue;
                    }
                    let sym = pick_symbol(&rule);
                    let bars = list_recent_bars(&pool, &bar_ex, &bar_seg, &sym, &bar_iv, 1)
                        .await
                        .ok()
                        .and_then(|b| b.into_iter().next());
                    let Some(mark) = bars.map(|b| b.close) else {
                        tracing::debug!(%sym, "copy_trade: bar / mark yok");
                        continue;
                    };
                    let qty = base_order_qty() * rule.size_multiplier;
                    let Some(qty) = apply_notional_bounds(&rule, qty, mark) else {
                        continue;
                    };
                    let qty = clamp_qty_by_max_notional_usdt(qty, mark);
                    if qty <= Decimal::ZERO {
                        continue;
                    }
                    let side = if score > th {
                        Some(OrderSide::Buy)
                    } else if score < -th {
                        Some(OrderSide::Sell)
                    } else {
                        None
                    };
                    let Some(side) = side else {
                        tracing::debug!(score, "copy_trade: eşik altı — yön yok");
                        continue;
                    };
                    let intent = OrderIntent {
                        instrument: InstrumentId {
                            exchange: ExchangeId::Binance,
                            segment: MarketSegment::Futures,
                            symbol: sym.clone(),
                        },
                        side,
                        quantity: qty,
                        order_type: OrderType::Market,
                        time_in_force: TimeInForce::Gtc,
                        requires_human_approval: requires_human_approval(),
                        futures: None,
                    };
                    let _ = gateway.set_reference_price(&intent.instrument, mark);
                    if auto_place() {
                        match gateway.place(intent).await {
                            Ok(id) => info!(sub_id = %r.id, %sym, ?side, %id, "copy_trade: emir"),
                            Err(e) => warn!(sub_id = %r.id, %sym, %e, "copy_trade: place"),
                        }
                    } else {
                        info!(
                            sub_id = %r.id,
                            %sym,
                            ?side,
                            score,
                            "copy_trade: sinyal (QTSS_COPY_TRADE_STRATEGY_AUTO_PLACE=1 ile yürüt)"
                        );
                    }
                }
            }
            Err(e) => warn!(%e, "copy_trade list_active_subscriptions"),
        }
    }
}
