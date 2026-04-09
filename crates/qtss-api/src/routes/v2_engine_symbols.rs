//! `GET/POST/PATCH/DELETE /v2/engine-symbols` — manuel engine_symbols
//! kayıt yönetimi.
//!
//! Web GUI'den operatörün yeni sembol/timeframe satırı eklemesi,
//! enable/disable etmesi veya silmesi için kullanılan CRUD endpoint'i.
//! Mevcut storage helper'ları (`insert_engine_symbol`,
//! `list_engine_symbols_all`, `update_engine_symbol_patch`,
//! `delete_engine_symbol`) üzerinden çalışır — yeni SQL yok.

use axum::extract::{Path, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::{
    delete_engine_symbol, insert_engine_symbol, list_engine_symbols_all,
    update_engine_symbol_patch, EngineSymbolInsert, EngineSymbolRow,
};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct EngineSymbolEntry {
    pub id: Uuid,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub enabled: bool,
    pub label: Option<String>,
    pub signal_direction_mode: String,
    pub lifecycle_state: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct EngineSymbolFeed {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<EngineSymbolEntry>,
}

#[derive(Debug, Deserialize)]
pub struct EngineSymbolCreate {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub label: Option<String>,
    pub signal_direction_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EngineSymbolPatch {
    pub enabled: Option<bool>,
    pub signal_direction_mode: Option<String>,
}

pub fn v2_engine_symbols_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/engine-symbols", get(list_handler).post(create_handler))
        .route(
            "/v2/engine-symbols/{id}",
            patch(patch_handler).delete(delete_handler),
        )
}

async fn list_handler(
    State(st): State<SharedState>,
) -> Result<Json<EngineSymbolFeed>, ApiError> {
    let rows = list_engine_symbols_all(&st.pool).await?;
    Ok(Json(EngineSymbolFeed {
        generated_at: Utc::now(),
        entries: rows.into_iter().map(row_to_entry).collect(),
    }))
}

async fn create_handler(
    State(st): State<SharedState>,
    Json(req): Json<EngineSymbolCreate>,
) -> Result<Json<EngineSymbolEntry>, ApiError> {
    // Light validation — DB CHECK constraints + ON CONFLICT DO UPDATE
    // do the rest of the heavy lifting (idempotent insert).
    let exchange = req.exchange.trim();
    let segment = req.segment.trim();
    let symbol = req.symbol.trim();
    let interval = req.interval.trim();
    if exchange.is_empty() || segment.is_empty() || symbol.is_empty() || interval.is_empty() {
        return Err(ApiError::bad_request(
            "exchange, segment, symbol, interval boş olamaz",
        ));
    }
    let row = insert_engine_symbol(
        &st.pool,
        &EngineSymbolInsert {
            exchange: exchange.to_string(),
            segment: segment.to_string(),
            symbol: symbol.to_string(),
            interval: interval.to_string(),
            label: req.label.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            signal_direction_mode: req
                .signal_direction_mode
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        },
    )
    .await?;
    Ok(Json(row_to_entry(row)))
}

async fn patch_handler(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<EngineSymbolPatch>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let touched = update_engine_symbol_patch(
        &st.pool,
        id,
        req.enabled,
        req.signal_direction_mode.as_deref(),
    )
    .await?;
    if touched == 0 {
        return Err(ApiError::not_found(format!("engine_symbol {id} bulunamadı")));
    }
    Ok(Json(serde_json::json!({ "updated": touched })))
}

async fn delete_handler(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let removed = delete_engine_symbol(&st.pool, id).await?;
    if removed == 0 {
        return Err(ApiError::not_found(format!("engine_symbol {id} bulunamadı")));
    }
    Ok(Json(serde_json::json!({ "deleted": removed })))
}

fn row_to_entry(r: EngineSymbolRow) -> EngineSymbolEntry {
    EngineSymbolEntry {
        id: r.id,
        exchange: r.exchange,
        segment: r.segment,
        symbol: r.symbol,
        interval: r.interval,
        enabled: r.enabled,
        label: r.label,
        signal_direction_mode: r.signal_direction_mode,
        lifecycle_state: r.lifecycle_state,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}
