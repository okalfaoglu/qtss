//! Backtest API — minimal MVP: run a local backtest over `market_bars`.

use std::collections::VecDeque;
use std::str::FromStr;

use axum::extract::{Extension, State};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use qtss_backtest::{BacktestConfig, BacktestEngine, BacktestResult, Strategy};
use qtss_chart_patterns::{
    analyze_trading_range, compute_signal_dashboard_v1_with_policy, OhlcBar, SignalDirectionPolicy,
    TradingRangeParams,
};
use qtss_domain::bar::TimestampBar;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::OrderSide;
use qtss_domain::symbol::InstrumentId;
use qtss_storage::list_bars_in_range;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

pub fn backtest_router() -> Router<SharedState> {
    Router::new().route("/backtest/run", post(backtest_run))
}

#[derive(Deserialize)]
struct BacktestRunBody {
    /// Strategy id: `buy_and_hold` | `sma_cross` | `trading_range`
    pub strategy: String,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub initial_equity: Decimal,
    #[serde(default)]
    pub sma_fast: Option<usize>,
    #[serde(default)]
    pub sma_slow: Option<usize>,
    /// İsteğe bağlı; yoksa motor varsayılanı (`BacktestConfig`).
    #[serde(default)]
    pub slippage_bps: Option<u32>,
    #[serde(default)]
    pub taker_fee_bps: Option<u32>,
    /// Linear futures leverage (isolated margin = notional / leverage). Spot için yok sayılır.
    #[serde(default)]
    pub leverage: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize)]
struct BacktestRunMeta {
    exchange: String,
    segment: String,
    symbol: String,
    interval: String,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    bar_count: usize,
    strategy: String,
    initial_equity: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    sma_fast: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sma_slow: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    leverage: Option<Decimal>,
    pub taker_fee_bps: u32,
    pub slippage_bps: u32,
}

#[derive(Debug, Clone, Serialize)]
struct BacktestRunResponse {
    meta: BacktestRunMeta,
    #[serde(flatten)]
    result: BacktestResult,
}

struct BuyAndHold {
    entered: bool,
}

impl BuyAndHold {
    fn new() -> Self {
        Self { entered: false }
    }
}

impl Strategy for BuyAndHold {
    fn name(&self) -> &'static str {
        "buy_and_hold"
    }

    fn on_bar(&mut self, ctx: &mut qtss_backtest::engine::BacktestContext, bar: &TimestampBar) {
        if self.entered {
            return;
        }
        if bar.close <= Decimal::ZERO {
            return;
        }
        let qty = ctx.max_order_qty_base(bar.close);
        if qty <= Decimal::ZERO {
            return;
        }
        let _ = ctx.market_order(
            OrderSide::Buy,
            qty,
            bar.close,
            bar.ts,
            ctx.slippage_bps,
            ctx.taker_fee_bps,
        );
        self.entered = true;
    }
}

struct SmaCross {
    fast: usize,
    slow: usize,
    closes: VecDeque<Decimal>,
    last_fast: Option<Decimal>,
    last_slow: Option<Decimal>,
}

impl SmaCross {
    fn new(fast: usize, slow: usize) -> Self {
        Self {
            fast: fast.max(1),
            slow: slow.max(2),
            closes: VecDeque::new(),
            last_fast: None,
            last_slow: None,
        }
    }

    fn sma(&self, n: usize) -> Option<Decimal> {
        if self.closes.len() < n {
            return None;
        }
        let mut sum = Decimal::ZERO;
        for x in self.closes.iter().rev().take(n) {
            sum += *x;
        }
        Some(sum / Decimal::from(n as u32))
    }
}

fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(f64::NAN)
}

/// Normalize client `strategy` strings to engine ids: `buy_and_hold` | `sma_cross` | `trading_range`.
fn canonical_backtest_strategy(raw: &str) -> Option<&'static str> {
    let s = raw.trim().to_lowercase().replace('-', "_");
    match s.as_str() {
        "buy_and_hold" | "buyandhold" => Some("buy_and_hold"),
        "sma_cross" | "smacross" => Some("sma_cross"),
        "trading_range" | "trading" | "signal_dashboard" | "signaldashboard" => Some("trading_range"),
        _ => None,
    }
}

/// Trading range + `signal_dashboard` **durum**: LONG / SHORT (politikaya göre) / NOTR flat.
/// İki yönlü marj benzeri kasa: long ve short `market_order` ile; komisyon `BacktestConfig::taker_fee_bps` + slip.
struct TradingRangeDurum {
    ohlc: Vec<OhlcBar>,
    params: TradingRangeParams,
    policy: SignalDirectionPolicy,
}

impl TradingRangeDurum {
    fn new(segment_lower: &str) -> Self {
        let seg = segment_lower.trim().to_lowercase();
        let policy = match seg.as_str() {
            "futures" | "usdt_futures" | "fapi" | "future" => SignalDirectionPolicy::Both,
            _ => SignalDirectionPolicy::LongOnly,
        };
        Self {
            ohlc: Vec::new(),
            params: TradingRangeParams::default(),
            policy,
        }
    }
}

impl Strategy for TradingRangeDurum {
    fn name(&self) -> &'static str {
        "trading_range"
    }

    fn on_bar(&mut self, ctx: &mut qtss_backtest::engine::BacktestContext, bar: &TimestampBar) {
        let idx = self.ohlc.len() as i64;
        self.ohlc.push(OhlcBar {
            open: decimal_to_f64(bar.open),
            high: decimal_to_f64(bar.high),
            low: decimal_to_f64(bar.low),
            close: decimal_to_f64(bar.close),
            bar_index: idx,
            volume: None,
        });
        let need = self.params.lookback.max(5) + 2;
        if self.ohlc.len() < need {
            return;
        }
        let tr = analyze_trading_range(&self.ohlc, &self.params);
        let dash = compute_signal_dashboard_v1_with_policy(&self.ohlc, &tr, self.policy);
        let durum = dash.durum.as_str();
        let slip = ctx.slippage_bps;
        let fee_bps = ctx.taker_fee_bps;

        if durum != "LONG" {
            if let Some(pos) = &ctx.position {
                if pos.side == OrderSide::Buy {
                    let qty = pos.qty;
                    let _ = ctx.market_order(OrderSide::Sell, qty, bar.close, bar.ts, slip, fee_bps);
                }
            }
        }
        if durum != "SHORT" {
            if let Some(pos) = &ctx.position {
                if pos.side == OrderSide::Sell {
                    let qty = pos.qty;
                    let _ = ctx.market_order(OrderSide::Buy, qty, bar.close, bar.ts, slip, fee_bps);
                }
            }
        }

        if bar.close <= Decimal::ZERO {
            return;
        }
        let qty = ctx.max_order_qty_base(bar.close);
        if qty <= Decimal::ZERO {
            return;
        }

        if durum == "LONG" && ctx.position.is_none() {
            let _ = ctx.market_order(OrderSide::Buy, qty, bar.close, bar.ts, slip, fee_bps);
        } else if durum == "SHORT" && ctx.position.is_none() {
            let _ = ctx.market_order(OrderSide::Sell, qty, bar.close, bar.ts, slip, fee_bps);
        }
    }
}

impl Strategy for SmaCross {
    fn name(&self) -> &'static str {
        "sma_cross"
    }

    fn on_bar(&mut self, ctx: &mut qtss_backtest::engine::BacktestContext, bar: &TimestampBar) {
        self.closes.push_back(bar.close);
        while self.closes.len() > self.slow * 4 {
            self.closes.pop_front();
        }
        let f = self.sma(self.fast);
        let s = self.sma(self.slow);
        let (Some(fast), Some(slow)) = (f, s) else {
            self.last_fast = f;
            self.last_slow = s;
            return;
        };
        let prev_fast = self.last_fast.unwrap_or(fast);
        let prev_slow = self.last_slow.unwrap_or(slow);
        self.last_fast = Some(fast);
        self.last_slow = Some(slow);

        let cross_up = prev_fast <= prev_slow && fast > slow;
        let cross_down = prev_fast >= prev_slow && fast < slow;

        let slip = ctx.slippage_bps;
        let fee_bps = ctx.taker_fee_bps;
        if cross_up && ctx.position.is_none() && bar.close > Decimal::ZERO {
            let qty = ctx.max_order_qty_base(bar.close);
            if qty > Decimal::ZERO {
                let _ = ctx.market_order(OrderSide::Buy, qty, bar.close, bar.ts, slip, fee_bps);
            }
        } else if cross_down && ctx.position.is_some() {
            if let Some(pos) = &ctx.position {
                if pos.side == OrderSide::Buy {
                    let qty = pos.qty;
                    let _ = ctx.market_order(OrderSide::Sell, qty, bar.close, bar.ts, slip, fee_bps);
                }
            }
        }
    }
}

async fn backtest_run(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<BacktestRunBody>,
) -> Result<Json<BacktestRunResponse>, ApiError> {
    let exchange = body.exchange.trim().to_lowercase();
    let segment = body.segment.trim().to_lowercase();
    let symbol = body.symbol.trim().to_uppercase();
    let interval = body.interval.trim().to_string();
    if exchange.is_empty() || segment.is_empty() || symbol.is_empty() || interval.is_empty() {
        return Err(ApiError::bad_request(
            "exchange/segment/symbol/interval boş olamaz",
        ));
    }
    if body.end_time < body.start_time {
        return Err(ApiError::bad_request("end_time >= start_time olmalı"));
    }

    let lim = 200_000_i64;
    let bars = list_bars_in_range(
        &st.pool,
        &exchange,
        &segment,
        &symbol,
        &interval,
        body.start_time,
        body.end_time,
        lim,
    )
    .await?;
    if bars.len() < 5 {
        return Err(ApiError::bad_request(
            "bar aralığında yeterli veri yok (market_bars)",
        ));
    }
    let bar_count = bars.len();

    let ex_id = ExchangeId::from_str(&exchange).unwrap_or(ExchangeId::Binance);
    let seg_id = match segment.as_str() {
        "futures" | "usdt_futures" | "fapi" | "future" | "perp" | "perpetual" => {
            MarketSegment::Futures
        }
        _ => MarketSegment::Spot,
    };
    let instrument = InstrumentId {
        exchange: ex_id,
        segment: seg_id,
        symbol: symbol.clone(),
    };

    let mut q: VecDeque<TimestampBar> = VecDeque::new();
    for b in bars {
        q.push_back(TimestampBar {
            ts: b.open_time,
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
        });
    }

    let mut cfg = BacktestConfig::default();
    cfg.initial_equity = body.initial_equity;
    if let Some(s) = body.slippage_bps {
        cfg.slippage_bps = s.min(500);
    }
    if let Some(f) = body.taker_fee_bps {
        cfg.taker_fee_bps = f.min(500);
    }
    if seg_id == MarketSegment::Futures {
        if let Some(lev) = body.leverage {
            let l = lev.max(Decimal::ONE).min(Decimal::from(125));
            cfg.max_leverage = l;
        }
    }
    let slippage_bps = cfg.slippage_bps;
    let taker_fee_bps = cfg.taker_fee_bps;
    let leverage_used = if seg_id == MarketSegment::Futures {
        Some(cfg.max_leverage)
    } else {
        None
    };
    let eng = BacktestEngine::new(cfg);

    let strat_id = canonical_backtest_strategy(&body.strategy).ok_or_else(|| {
        ApiError::bad_request(
            "strategy: buy_and_hold | sma_cross | trading_range (aliases: trading-range, trading, signal_dashboard)",
        )
    })?;
    let mut strat_box: Box<dyn Strategy> = match strat_id {
        "buy_and_hold" => Box::new(BuyAndHold::new()),
        "sma_cross" => {
            let fast = body.sma_fast.unwrap_or(10).clamp(1, 500);
            let slow = body.sma_slow.unwrap_or(30).clamp(2, 500);
            if fast >= slow {
                return Err(ApiError::bad_request("sma_fast < sma_slow olmalı"));
            }
            Box::new(SmaCross::new(fast, slow))
        }
        "trading_range" => Box::new(TradingRangeDurum::new(&segment)),
        _ => {
            return Err(ApiError::bad_request(
                "strategy: buy_and_hold | sma_cross | trading_range",
            ));
        }
    };

    let res = eng.run(instrument, q, strat_box.as_mut());
    Ok(Json(BacktestRunResponse {
        meta: BacktestRunMeta {
            exchange,
            segment,
            symbol,
            interval,
            start_time: body.start_time,
            end_time: body.end_time,
            bar_count,
            strategy: strat_id.to_string(),
            initial_equity: body.initial_equity,
            sma_fast: body.sma_fast,
            sma_slow: body.sma_slow,
            leverage: leverage_used,
            taker_fee_bps,
            slippage_bps,
        },
        result: res,
    }))
}

