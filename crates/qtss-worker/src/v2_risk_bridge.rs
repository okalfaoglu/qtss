//! v2 intent → risk bridge — Faz 7 Adım 7.
//!
//! Subscribes to `INTENT_CREATED`, runs every incoming `TradeIntent`
//! through a configured `RiskEngine`, and publishes the outcome on
//! either `INTENT_APPROVED` (with the resulting `ApprovedIntent`) or
//! `INTENT_REJECTED` (with the `RiskRejection` reason).
//!
//! Why a separate bridge: CLAUDE.md #3 — strategy emits intents, risk
//! gates them, execution consumes approvals. Each layer is wired
//! through the in-process bus so any one of them can be replaced or
//! disabled without touching the others.
//!
//! ## Bootstrap shortcuts
//!
//! Two pieces are still stubbed because the upstream wiring has not
//! landed yet:
//!
//! * **AccountState**: built from a tiny config-driven snapshot
//!   (`risk.bootstrap.equity` etc.). Once `qtss-portfolio` exposes a
//!   live snapshot we swap that in here without touching the rest of
//!   the loop.
//! * **TradeIntent.entry_price**: the rule strategy emits market entries
//!   (`None`) but `RiskEngine.approve` requires a concrete price to
//!   compute notional. We synthesise an entry from the latest closed
//!   bar in `market_bars` so the gate has something to score against.

use std::sync::Arc;

use qtss_domain::v2::intent::{ApprovedIntent, RiskRejection, TradeIntent};
use qtss_eventbus::{
    topics::{INTENT_APPROVED, INTENT_CREATED, INTENT_REJECTED},
    EventBus, EventBusError, InProcessBus,
};
use qtss_risk::{
    AccountState, DailyLossCheck, DrawdownCheck, KillSwitchCheck, LeverageCheck,
    MaxOpenPositionsCheck, RiskConfig, RiskEngine, SizerRegistry, StopDistanceCheck,
};
use qtss_storage::{list_recent_bars, resolve_system_f64};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use tracing::{info, warn};

pub async fn v2_risk_bridge_loop(pool: PgPool, bus: Arc<InProcessBus>) {
    info!("v2 risk bridge spawned (subscribes to {INTENT_CREATED})");

    let engine = match build_engine() {
        Ok(e) => e,
        Err(e) => {
            warn!(%e, "v2 risk bridge: engine init failed, exiting");
            return;
        }
    };

    let mut stream = bus.subscribe::<TradeIntent>(INTENT_CREATED);
    loop {
        match stream.recv().await {
            Ok(Some(event)) => {
                let intent = event.payload;
                handle(&pool, &engine, &bus, intent).await;
            }
            Ok(None) => {}
            Err(EventBusError::Lagged { topic, skipped }) => {
                warn!(%topic, skipped, "v2 risk bridge lagged, resuming");
            }
            Err(e) => {
                warn!(%e, "v2 risk bridge stream closed");
                return;
            }
        }
    }
}

async fn handle(
    pool: &PgPool,
    engine: &RiskEngine,
    bus: &InProcessBus,
    mut intent: TradeIntent,
) {
    if intent.entry_price.is_none() {
        if let Some(px) = latest_close(pool, &intent).await {
            intent.entry_price = Some(px);
        }
    }
    let state = bootstrap_account_state(pool).await;
    match engine.approve(intent.clone(), &state) {
        Ok(approved) => {
            info!(
                strategy = %intent.strategy_id,
                symbol = %intent.instrument.symbol,
                qty = %approved.quantity,
                notional = %approved.notional,
                "v2 risk bridge: approved"
            );
            publish(bus, INTENT_APPROVED, &approved).await;
        }
        Err(rejection) => {
            info!(
                strategy = %intent.strategy_id,
                symbol = %intent.instrument.symbol,
                ?rejection,
                "v2 risk bridge: rejected"
            );
            publish(bus, INTENT_REJECTED, &RejectedEnvelope { intent, rejection }).await;
        }
    }
}

async fn publish<T: Serialize + Send + Sync>(bus: &InProcessBus, topic: &str, payload: &T) {
    if let Err(e) = bus.publish(topic, payload).await {
        warn!(topic = %topic, %e, "v2 risk bridge: publish failed");
    }
}

#[derive(Debug, Clone, Serialize)]
struct RejectedEnvelope {
    intent: TradeIntent,
    rejection: RiskRejection,
}

// Compile-time anchor: ApprovedIntent must stay on the bus signature.
const _: fn(&ApprovedIntent) = |_| {};

/// Build the engine with the default check + sizer set. Tunables come
/// from `system_config`; the bootstrap defaults below match the
/// hardcoded defaults already living in `qtss-risk` so a fresh deploy
/// behaves identically until an operator overrides them.
fn build_engine() -> anyhow::Result<RiskEngine> {
    let config = RiskConfig::defaults();
    let mut engine = RiskEngine::new(config, SizerRegistry::with_defaults())
        .map_err(|e| anyhow::anyhow!("RiskEngine::new failed: {e}"))?;
    engine.register_check(Arc::new(KillSwitchCheck));
    engine.register_check(Arc::new(DrawdownCheck));
    engine.register_check(Arc::new(DailyLossCheck));
    engine.register_check(Arc::new(MaxOpenPositionsCheck));
    engine.register_check(Arc::new(LeverageCheck));
    engine.register_check(Arc::new(StopDistanceCheck));
    Ok(engine)
}

/// Bootstrap account state. Reads `risk.bootstrap.equity` from
/// `system_config` so an operator can adjust the simulated bankroll
/// without a deploy (CLAUDE.md #2). Replaced by a real portfolio
/// snapshot once `qtss-portfolio` exposes one.
async fn bootstrap_account_state(pool: &PgPool) -> AccountState {
    let equity_f = resolve_system_f64(
        pool,
        "risk",
        "bootstrap.equity",
        "QTSS_RISK_BOOTSTRAP_EQUITY",
        10_000.0,
    )
    .await;
    let equity = Decimal::from_f64(equity_f).unwrap_or_else(|| Decimal::from(10_000));
    AccountState {
        equity,
        peak_equity: equity,
        day_pnl: Decimal::ZERO,
        open_positions: 0,
        current_leverage: Decimal::ZERO,
        kill_switch_manual: false,
    }
}

/// Pull the most recent bar's close for the intent's instrument so we
/// have something to plug into `entry_price`. The orchestrator already
/// proves bars are present (it walks the same table); if the lookup
/// fails we leave entry_price `None` and let the engine reject.
async fn latest_close(pool: &PgPool, intent: &TradeIntent) -> Option<Decimal> {
    let exchange = match &intent.instrument.venue {
        qtss_domain::v2::instrument::Venue::Binance => "binance",
        qtss_domain::v2::instrument::Venue::Bybit => "bybit",
        qtss_domain::v2::instrument::Venue::Okx => "okx",
        qtss_domain::v2::instrument::Venue::Bist => "bist",
        qtss_domain::v2::instrument::Venue::Nasdaq => "nasdaq",
        qtss_domain::v2::instrument::Venue::Nyse => "nyse",
        qtss_domain::v2::instrument::Venue::Polygon => "polygon",
        qtss_domain::v2::instrument::Venue::Alpaca => "alpaca",
        qtss_domain::v2::instrument::Venue::Ibkr => "ibkr",
        qtss_domain::v2::instrument::Venue::Custom(s) => s.as_str(),
    };
    let segment = match intent.instrument.asset_class {
        qtss_domain::v2::instrument::AssetClass::CryptoFutures => "futures",
        _ => "spot",
    };
    let interval = format!("{:?}", intent.timeframe).to_lowercase();
    // Timeframe::Debug yields "M1"/"H4"/etc. Map to "1m"/"4h" the same
    // way the orchestrator parses it on the way in.
    let interval = match interval.as_str() {
        "m1" => "1m",
        "m3" => "3m",
        "m5" => "5m",
        "m15" => "15m",
        "m30" => "30m",
        "h1" => "1h",
        "h2" => "2h",
        "h4" => "4h",
        "h6" => "6h",
        "h8" => "8h",
        "h12" => "12h",
        "d1" => "1d",
        "d3" => "3d",
        "w1" => "1w",
        "mn1" => "1mo",
        _ => return None,
    };
    match list_recent_bars(pool, exchange, segment, &intent.instrument.symbol, interval, 1).await {
        Ok(rows) => rows.first().map(|r| r.close),
        Err(e) => {
            warn!(%e, "v2 risk bridge: latest_close lookup failed");
            None
        }
    }
}
