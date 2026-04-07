//! Wraps [`DryRunGateway`] and mirrors each simulated fill to `exchange_orders` only (no `paper_*` tables).
//!
//! Used when `worker.paper_ledger_enabled` is off but `paper_org_id` / `paper_user_id` are valid UUIDs
//! so dry strategy fills still appear in the same ledger as live and AI dry rows.

use std::sync::Arc;

use async_trait::async_trait;
use qtss_domain::symbol::InstrumentId;
use qtss_execution::{DryRunGateway, ExecutionError, ExecutionGateway};
use qtss_storage::{resolve_system_string, ExchangeOrderRepository};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

fn exchange_code(e: qtss_domain::exchange::ExchangeId) -> &'static str {
    match e {
        qtss_domain::exchange::ExchangeId::Binance => "binance",
        qtss_domain::exchange::ExchangeId::Bybit => "bybit",
        qtss_domain::exchange::ExchangeId::Okx => "okx",
        qtss_domain::exchange::ExchangeId::Custom => "custom",
    }
}

fn segment_code(s: qtss_domain::exchange::MarketSegment) -> &'static str {
    match s {
        qtss_domain::exchange::MarketSegment::Spot => "spot",
        qtss_domain::exchange::MarketSegment::Futures => "futures",
        qtss_domain::exchange::MarketSegment::Margin => "margin",
        qtss_domain::exchange::MarketSegment::Options => "options",
    }
}

/// Parses `worker.paper_org_id` and `paper_user_id` as UUIDs. Does not require `paper_ledger_enabled`.
pub async fn paper_actor_uuids_from_db(pool: &PgPool) -> Option<(Uuid, Uuid)> {
    let org_s = resolve_system_string(pool, "worker", "paper_org_id", "QTSS_PAPER_ORG_ID", "").await;
    let user_s =
        resolve_system_string(pool, "worker", "paper_user_id", "QTSS_PAPER_USER_ID", "").await;
    let org = Uuid::parse_str(org_s.trim()).ok()?;
    let user = Uuid::parse_str(user_s.trim()).ok()?;
    Some((org, user))
}

pub struct DryOrdersMirrorGateway {
    dry: Arc<DryRunGateway>,
    pool: PgPool,
    org_id: Uuid,
    user_id: Uuid,
    simulation_source: String,
}

impl DryOrdersMirrorGateway {
    #[must_use]
    pub fn new(
        dry: Arc<DryRunGateway>,
        pool: PgPool,
        org_id: Uuid,
        user_id: Uuid,
        simulation_source: impl Into<String>,
    ) -> Self {
        Self {
            dry,
            pool,
            org_id,
            user_id,
            simulation_source: simulation_source.into(),
        }
    }
}

#[async_trait]
impl ExecutionGateway for DryOrdersMirrorGateway {
    fn set_reference_price(
        &self,
        instrument: &InstrumentId,
        price: Decimal,
    ) -> Result<(), ExecutionError> {
        self.dry.set_reference_price(instrument, price)
    }

    async fn place(&self, intent: qtss_domain::orders::OrderIntent) -> Result<Uuid, ExecutionError> {
        let out = self.dry.place_detailed(intent.clone(), None)?;
        let sym_u = intent.instrument.symbol.trim().to_uppercase();
        let venue = json!({
            "dry_run": true,
            "simulation_source": "dry_orders_mirror_gateway",
            "strategy_key": self.simulation_source,
            "status": "FILLED",
            "executedQty": out.fill.quantity.to_string(),
            "avgPrice": out.fill.avg_price.to_string(),
            "fee": out.fill.fee.to_string(),
            "note": "Dry strategy fill mirrored to exchange_orders; no venue HTTP.",
        });
        let repo = ExchangeOrderRepository::new(self.pool.clone());
        if let Err(e) = repo
            .insert_dry_simulated_filled(
                self.org_id,
                self.user_id,
                exchange_code(intent.instrument.exchange),
                segment_code(intent.instrument.segment),
                &sym_u,
                out.client_order_id,
                &intent,
                venue,
            )
            .await
        {
            warn!(
                %e,
                client_order_id = %out.client_order_id,
                source = %self.simulation_source,
                symbol = %sym_u,
                "dry_orders_mirror_gateway: exchange_orders insert failed (in-memory dry ledger updated)"
            );
        }
        Ok(out.client_order_id)
    }

    async fn cancel(&self, client_order_id: Uuid) -> Result<(), ExecutionError> {
        self.dry.cancel(client_order_id).await
    }
}
