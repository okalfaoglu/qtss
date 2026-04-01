//! Wraps [`DryRunGateway`] so each simulated fill is persisted to `paper_balances` / `paper_fills`.
//!
//! `worker.paper_ledger_enabled` + `paper_org_id` / `paper_user_id` (`system_config`). Used by the worker strategy runner.

use std::sync::Arc;

use async_trait::async_trait;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{OrderIntent, OrderSide};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::{DryPlaceOutcome, DryRunGateway, ExecutionError, ExecutionGateway};
use qtss_storage::{
    resolve_system_string, resolve_worker_enabled_flag, PaperLedgerRepository, StorageError,
};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::error;
use uuid::Uuid;

/// `worker.paper_*` — her iki UUID de geçerli olmalı.
pub async fn paper_ledger_target_from_db(pool: &PgPool) -> Option<(Uuid, Uuid)> {
    let on = resolve_worker_enabled_flag(
        pool,
        "worker",
        "paper_ledger_enabled",
        "QTSS_PAPER_LEDGER_ENABLED",
        false,
    )
    .await;
    if !on {
        return None;
    }
    let org_s = resolve_system_string(pool, "worker", "paper_org_id", "QTSS_PAPER_ORG_ID", "").await;
    let user_s =
        resolve_system_string(pool, "worker", "paper_user_id", "QTSS_PAPER_USER_ID", "").await;
    let org = Uuid::parse_str(org_s.trim()).ok()?;
    let user = Uuid::parse_str(user_s.trim()).ok()?;
    Some((org, user))
}

fn exchange_code(e: ExchangeId) -> &'static str {
    match e {
        ExchangeId::Binance => "binance",
        ExchangeId::Bybit => "bybit",
        ExchangeId::Okx => "okx",
        ExchangeId::Custom => "custom",
    }
}

fn segment_code(s: MarketSegment) -> &'static str {
    match s {
        MarketSegment::Spot => "spot",
        MarketSegment::Futures => "futures",
        MarketSegment::Margin => "margin",
        MarketSegment::Options => "options",
    }
}

fn side_code(side: OrderSide) -> &'static str {
    match side {
        OrderSide::Buy => "BUY",
        OrderSide::Sell => "SELL",
    }
}

/// Persists dry-run fills to PostgreSQL for dashboards and `paper_fill_notify`.
pub struct PaperRecordingDryGateway {
    dry: Arc<DryRunGateway>,
    pool: PgPool,
    org_id: Uuid,
    user_id: Uuid,
    /// Isolates `paper_balances` / `paper_fills` when several dry strategies share the same user.
    strategy_key: String,
}

impl PaperRecordingDryGateway {
    #[must_use]
    pub fn new(
        dry: Arc<DryRunGateway>,
        pool: PgPool,
        org_id: Uuid,
        user_id: Uuid,
        strategy_key: impl Into<String>,
    ) -> Self {
        Self {
            dry,
            pool,
            org_id,
            user_id,
            strategy_key: strategy_key.into(),
        }
    }

    async fn persist_fill(
        &self,
        intent: &OrderIntent,
        out: &DryPlaceOutcome,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let paper = PaperLedgerRepository::new(self.pool.clone());
        paper
            .upsert_balance_snapshot(
                &mut tx,
                self.org_id,
                self.user_id,
                self.strategy_key.as_str(),
                out.quote_balance_after,
                &out.base_positions_after,
            )
            .await?;
        paper
            .insert_fill(
                &mut tx,
                self.org_id,
                self.user_id,
                self.strategy_key.as_str(),
                exchange_code(intent.instrument.exchange),
                segment_code(intent.instrument.segment),
                intent.instrument.symbol.trim(),
                out.client_order_id,
                side_code(intent.side),
                out.fill.quantity,
                out.fill.avg_price,
                out.fill.fee,
                out.quote_balance_after,
                &out.base_positions_after,
                intent,
            )
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl ExecutionGateway for PaperRecordingDryGateway {
    fn set_reference_price(
        &self,
        instrument: &InstrumentId,
        price: Decimal,
    ) -> Result<(), ExecutionError> {
        self.dry.set_reference_price(instrument, price)
    }

    async fn place(&self, intent: OrderIntent) -> Result<Uuid, ExecutionError> {
        let out = self.dry.place_detailed(intent.clone(), None)?;
        if let Err(e) = self.persist_fill(&intent, &out).await {
            error!(
                %e,
                client_order_id = %out.client_order_id,
                "paper ledger persist failed after dry place (in-memory ledger already updated)"
            );
            return Err(ExecutionError::Other(format!("paper ledger: storage error — {e}")));
        }
        Ok(out.client_order_id)
    }

    async fn cancel(&self, client_order_id: Uuid) -> Result<(), ExecutionError> {
        self.dry.cancel(client_order_id).await
    }
}
