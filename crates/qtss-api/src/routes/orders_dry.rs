//! Paper / dry-run emir — borsaya gitmez; `paper_balances` + `paper_fills`.

use std::collections::HashMap;

use axum::extract::{Extension, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use qtss_domain::exchange::ExchangeId;
use qtss_domain::orders::{OrderIntent, OrderSide, OrderType};
use qtss_execution::{apply_place, CommissionPolicy, DryLedgerState};
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use qtss_storage::{PaperBalanceRow, PaperFillRow};

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct PlaceDryBody {
    pub intent: OrderIntent,
    /// `Market` emirleri için zorunlu. `Limit` için limit fiyatı kullanılır.
    pub mark_price: Option<Decimal>,
    /// İlk kayıtta kullanılacak başlangıç quote bakiyesi (varsayılan 10_000).
    pub initial_quote_balance: Option<Decimal>,
}

fn segment_db_key(
    segment: qtss_domain::exchange::MarketSegment,
) -> Result<&'static str, ApiError> {
    match segment {
        qtss_domain::exchange::MarketSegment::Spot => Ok("spot"),
        qtss_domain::exchange::MarketSegment::Futures => Ok("futures"),
        _ => Err(ApiError::bad_request("bu segment için paper emir kapalı")),
    }
}

fn exchange_db_key(ex: ExchangeId) -> &'static str {
    match ex {
        ExchangeId::Binance => "binance",
        ExchangeId::Custom => "custom",
    }
}

fn ledger_from_row(row: PaperBalanceRow) -> DryLedgerState {
    DryLedgerState {
        quote_balance: row.quote_balance,
        base_by_symbol: row.base_positions.0,
        marks: HashMap::new(),
    }
}

#[derive(Deserialize)]
pub struct PaperFillsQuery {
    /// 1–500; varsayılan 50.
    #[serde(default = "paper_fills_default_limit")]
    limit: i64,
}

fn paper_fills_default_limit() -> i64 {
    50
}

pub fn orders_dry_read_router() -> Router<SharedState> {
    Router::new()
        .route("/orders/dry/fills", get(list_paper_fills))
        .route("/orders/dry/balance", get(get_paper_balance))
}

pub fn orders_dry_write_router() -> Router<SharedState> {
    Router::new().route("/orders/dry/place", post(place_dry_order))
}

async fn get_paper_balance(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
) -> Result<Json<Option<PaperBalanceRow>>, String> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| "geçersiz token sub".to_string())?;
    let row = st
        .paper
        .fetch_balance(user_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(row))
}

async fn list_paper_fills(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<PaperFillsQuery>,
) -> Result<Json<Vec<PaperFillRow>>, ApiError> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    let lim = q.limit.clamp(1, 500);
    let rows = st.paper.list_fills_for_user(user_id, lim).await?;
    Ok(Json(rows))
}

async fn place_dry_order(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<PlaceDryBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let org_id = Uuid::parse_str(&claims.org_id)
        .map_err(|_| ApiError::bad_request("geçersiz token org_id"))?;
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("geçersiz token sub"))?;

    if matches!(body.intent.order_type, OrderType::Market) && body.mark_price.is_none() {
        return Err(ApiError::bad_request(
            "dry: Market emri için mark_price gerekli",
        ));
    }

    let seg = segment_db_key(body.intent.instrument.segment)?;
    let ex = exchange_db_key(body.intent.instrument.exchange);
    let symbol = body.intent.instrument.symbol.clone();

    let mut tx = st.pool.begin().await?;
    let locked = st
        .paper
        .lock_balance_for_update(&mut tx, user_id)
        .await?;

    let mut ledger = if let Some(r) = locked {
        ledger_from_row(r)
    } else {
        let init = body
            .initial_quote_balance
            .unwrap_or_else(|| Decimal::new(10_000, 0));
        let row = st
            .paper
            .insert_balance(&mut tx, org_id, user_id, init)
            .await?;
        ledger_from_row(row)
    };

    let policy = CommissionPolicy::default();
    let out = apply_place(
        &mut ledger,
        &policy,
        None,
        body.intent.clone(),
        body.mark_price,
    )
    .map_err(|e| ApiError::bad_request(e.to_string()))?;

    st.paper
        .update_balance(
            &mut tx,
            user_id,
            out.quote_balance_after,
            &out.base_positions_after,
        )
        .await?;

    let side_str = match body.intent.side {
        OrderSide::Buy => "buy",
        OrderSide::Sell => "sell",
    };

    st.paper
        .insert_fill(
            &mut tx,
            org_id,
            user_id,
            ex,
            seg,
            &symbol,
            out.client_order_id,
            side_str,
            out.fill.quantity,
            out.fill.avg_price,
            out.fill.fee,
            out.quote_balance_after,
            &out.base_positions_after,
            &body.intent,
        )
        .await?;

    tx.commit().await?;

    Ok(Json(serde_json::json!({
        "client_order_id": out.client_order_id,
        "avg_price": out.fill.avg_price.to_string(),
        "quantity": out.fill.quantity.to_string(),
        "fee": out.fill.fee.to_string(),
        "quote_balance_after": out.quote_balance_after.to_string(),
    })))
}
