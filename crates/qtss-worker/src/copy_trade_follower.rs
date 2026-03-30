//! Aktif copy abonelikleri + Nansen perp yönü → isteğe bağlı paper emir (dev guide §3.4).
//!
//! - `CopyRule.max_latency_ms`: Nansen `data_snapshots` paketinin en eski satırına göre gecikme.
//! - `QTSS_COPY_TRADE_FOLLOWER_AUTO_PLACE=1` → [`strategy_runner::dry_gateway_from_env`] ile market emri (dry defter).

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use qtss_common::is_trading_halted;
use qtss_domain::copy_trade::CopyRule;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{OrderIntent, OrderSide, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::ExecutionGateway;
use qtss_storage::{
    data_snapshot_age_secs, fetch_data_snapshot, list_recent_bars, CopySubscriptionRepository,
    CopySubscriptionRow,
};
use qtss_strategy::risk::{apply_kelly_scale_to_qty, clamp_qty_by_max_notional_usdt};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::data_sources::registry::REGISTERED_NANSEN_HTTP_KEYS;
use crate::signal_scorer::score_nansen_perp_direction;
use crate::strategy_runner::dry_gateway_from_env;

const NANSEN_PERP_TRADES_KEY: &str = "nansen_perp_trades";

fn enabled() -> bool {
    std::env::var("QTSS_COPY_TRADE_FOLLOWER_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn auto_place() -> bool {
    std::env::var("QTSS_COPY_TRADE_FOLLOWER_AUTO_PLACE")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn tick_secs() -> u64 {
    std::env::var("QTSS_COPY_TRADE_FOLLOWER_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300)
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

fn requires_human_approval() -> bool {
    !std::env::var("QTSS_STRATEGY_SKIP_HUMAN_APPROVAL")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

/// En kötü Nansen snapshot yaşı (ms): tüm kayıtlı anahtarlar için `max(age)`; eksik anahtar → `None`.
async fn nansen_snapshot_bundle_age_ms(pool: &PgPool) -> Option<i64> {
    let mut max_secs: i64 = 0;
    let mut any = false;
    for key in REGISTERED_NANSEN_HTTP_KEYS {
        match data_snapshot_age_secs(pool, key).await {
            Ok(Some(secs)) => {
                if secs < 0 {
                    return None;
                }
                any = true;
                max_secs = max_secs.max(secs);
            }
            Ok(None) => return None,
            Err(e) => {
                warn!(%key, %e, "copy_trade_follower: data_snapshot_age_secs");
            }
        }
    }
    any.then_some(max_secs.saturating_mul(1000))
}

fn rule_parse(v: &serde_json::Value) -> Option<CopyRule> {
    serde_json::from_value(v.clone()).ok()
}

async fn nansen_fresh_for_rule(pool: &PgPool, rule: &CopyRule) -> bool {
    if rule.max_latency_ms <= 0 {
        return true;
    }
    let Some(age_ms) = nansen_snapshot_bundle_age_ms(pool).await else {
        warn!(
            max_ms = rule.max_latency_ms,
            "copy_trade_follower: Nansen snapshot eksik — max_latency aşılamadı (atlanıyor)"
        );
        return false;
    };
    if age_ms > rule.max_latency_ms {
        warn!(
            age_ms,
            max_ms = rule.max_latency_ms,
            "copy_trade_follower: Nansen verisi max_latency üzerinde — tick atlandı"
        );
        return false;
    }
    true
}

fn pick_symbol(rule: &CopyRule) -> String {
    rule.symbol_allowlist
        .iter()
        .map(|x| x.trim().to_uppercase())
        .find(|x| !x.is_empty())
        .unwrap_or_else(default_symbol)
}

fn apply_notional_bounds(rule: &CopyRule, qty: Decimal, mark: Decimal) -> Option<Decimal> {
    let notional = qty * mark;
    if let Some(mn) = rule.min_notional {
        if notional < mn {
            warn!(%notional, min = %mn, "copy_trade_follower: min_notional altında");
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

async fn perp_snapshot_json(pool: &PgPool) -> Option<Value> {
    match fetch_data_snapshot(pool, NANSEN_PERP_TRADES_KEY).await {
        Ok(r) => r.and_then(|row| row.response_json),
        Err(e) => {
            warn!(%e, "copy_trade_follower: fetch_data_snapshot nansen_perp_trades");
            None
        }
    }
}

async fn place_for_subscriptions(
    pool: &PgPool,
    gateway: &Arc<dyn ExecutionGateway>,
    perp_json: &Value,
    rows: &[CopySubscriptionRow],
    bar_ex: &str,
    bar_seg: &str,
    bar_iv: &str,
) {
    let score = score_nansen_perp_direction(perp_json);
    let th = direction_threshold();
    for sub in rows {
        let Some(rule) = rule_parse(&sub.rule) else {
            warn!(id = %sub.id, "copy_trade_follower: CopyRule ayrıştırılamadı");
            continue;
        };
        if !nansen_fresh_for_rule(pool, &rule).await {
            continue;
        }
        let sym = pick_symbol(&rule);
        let mark = match list_recent_bars(pool, bar_ex, bar_seg, &sym, bar_iv, 1).await {
            Ok(b) => b.into_iter().next().map(|x| x.close),
            Err(e) => {
                warn!(%sym, %e, "copy_trade_follower: list_recent_bars");
                None
            }
        };
        let Some(mark) = mark else {
            tracing::debug!(%sym, "copy_trade_follower: mark yok");
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
            tracing::debug!(score, "copy_trade_follower: eşik altı");
            continue;
        };
        let inst = InstrumentId {
            exchange: ExchangeId::Binance,
            segment: MarketSegment::Futures,
            symbol: sym.clone(),
        };
        let intent = OrderIntent {
            instrument: inst.clone(),
            side,
            quantity: qty,
            order_type: OrderType::Market,
            time_in_force: TimeInForce::Gtc,
            requires_human_approval: requires_human_approval(),
            futures: None,
        };
        let _ = gateway.set_reference_price(&inst, mark);
        match gateway.place(intent).await {
            Ok(id) => info!(sub_id = %sub.id, %sym, ?side, %id, "copy_trade_follower: emir"),
            Err(e) => warn!(sub_id = %sub.id, %sym, %e, "copy_trade_follower: place"),
        }
    }
}

pub async fn copy_trade_follower_loop(pool: PgPool) {
    if !enabled() {
        info!("QTSS_COPY_TRADE_FOLLOWER_ENABLED kapalı — copy_trade_follower_loop çıkıyor");
        return;
    }
    let tick = Duration::from_secs(tick_secs());
    let repo = CopySubscriptionRepository::new(pool.clone());
    let gw: Option<Arc<dyn ExecutionGateway>> = if auto_place() {
        Some(dry_gateway_from_env() as Arc<dyn ExecutionGateway>)
    } else {
        None
    };
    let (bar_ex, bar_seg, bar_iv) = bar_ctx();
    info!(
        poll_secs = tick.as_secs(),
        auto_place = auto_place(),
        "copy_trade_follower_loop (abonelik + Nansen gecikme + isteğe bağlı place)"
    );
    loop {
        tokio::time::sleep(tick).await;
        if is_trading_halted() {
            tracing::debug!("copy_trade_follower: kill switch — place yok");
            continue;
        }
        match repo.list_active_subscriptions().await {
            Ok(rows) => {
                if rows.is_empty() {
                    tracing::debug!("copy_trade_follower: aktif abonelik yok");
                    continue;
                }
                if let Some(ref g) = gw {
                    if let Some(j) = perp_snapshot_json(&pool).await {
                        place_for_subscriptions(&pool, g, &j, &rows, &bar_ex, &bar_seg, &bar_iv)
                            .await;
                    }
                } else {
                    for sub in &rows {
                        let Some(rule) = rule_parse(&sub.rule) else {
                            warn!(id = %sub.id, "copy_trade_follower: CopyRule ayrıştırılamadı");
                            continue;
                        };
                        if !nansen_fresh_for_rule(&pool, &rule).await {
                            continue;
                        }
                        let allow = if rule.symbol_allowlist.is_empty() {
                            "varsayılan sembol"
                        } else {
                            "allowlist"
                        };
                        tracing::debug!(
                            id = %sub.id,
                            max_latency_ms = rule.max_latency_ms,
                            symbols = %allow,
                            "copy_trade_follower: Nansen OK — QTSS_COPY_TRADE_FOLLOWER_AUTO_PLACE=1 ile emir"
                        );
                    }
                }
                info!(
                    count = rows.len(),
                    at = %Utc::now().to_rfc3339(),
                    "copy_trade_follower: tick tamam"
                );
            }
            Err(e) => warn!(%e, "copy_trade_follower: list_active_subscriptions"),
        }
    }
}
