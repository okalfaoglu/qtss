//! Wyckoff structure API endpoints (Faz 10).
//!
//! GET /v2/wyckoff/active            — all active structures
//! GET /v2/wyckoff/structure/:id     — single structure detail
//! GET /v2/wyckoff/history/:symbol   — symbol history
//! GET /v2/wyckoff/overlay/:symbol/:interval — chart overlay data

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::SharedState;
use qtss_storage::{
    find_active_wyckoff_structure, get_wyckoff_structure, list_active_wyckoff_structures,
    list_phase_groups_by_timeframe, list_recent_wyckoff_structures, list_wyckoff_history,
    v2_setups::{list_v2_setups_filtered, SetupFilter},
};
use qtss_wyckoff::structure::{
    validate_event_placement, WyckoffEvent, WyckoffPhase, WyckoffSchematic,
};

#[derive(Debug, Deserialize)]
pub struct ActiveQuery {
    pub symbol: Option<String>,
    pub interval: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct RecentQuery {
    /// `active` | `completed` | `failed` | `all` (default `all`).
    pub status: Option<String>,
    pub exchange: Option<String>,
    pub symbol: Option<String>,
    pub interval: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct PhaseGroupsQuery {
    pub exchange: Option<String>,
    pub symbol: Option<String>,
    pub interval: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WyckoffSetupsQuery {
    pub limit: Option<i64>,
    pub mode: Option<String>,
    pub state: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub profile: Option<String>,
    pub venue: Option<String>,
}

pub fn v2_wyckoff_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/wyckoff/active", get(get_active))
        .route("/v2/wyckoff/recent", get(get_recent))
        .route("/v2/wyckoff/structure/{id}", get(get_structure))
        .route("/v2/wyckoff/history/{symbol}", get(get_history))
        .route(
            "/v2/wyckoff/overlay/{symbol}/{interval}",
            get(get_overlay),
        )
        .route("/v2/wyckoff/setups", get(get_wyckoff_setups))
        .route("/v2/wyckoff/phase-groups", get(get_phase_groups))
        .route("/v2/wyckoff/events", get(get_events))
}

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    pub symbol: Option<String>,
    pub interval: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    /// `all` (default), `valid`, `violations` — server-side filter on
    /// the validator output so the GUI doesn't have to re-walk the list.
    pub kind: Option<String>,
}

/// Flat event feed for chart overlay + violation surfacing (Faz 10
/// follow-up to backlog `faz_wyckoff_event_labels_overlay.md`). Reads
/// active+recent structures, flattens `events_json`, and runs
/// `validate_event_placement` per event for direction/phase coherence
/// audit. The chart toggles markers from this feed; the Wyckoff page
/// surfaces violation counts.
async fn get_events(
    State(st): State<SharedState>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<JsonValue>, ApiError> {
    let limit = q.limit.unwrap_or(500).clamp(1, 5_000);
    // Pull a generous window of structures so even if the latest one is
    // light on events we still have enough to render. Internal cap = 100
    // structures (Wyckoff doesn't churn that fast).
    let rows = list_recent_wyckoff_structures(
        &st.pool,
        q.status.as_deref(),
        None,
        q.symbol.as_deref(),
        q.interval.as_deref(),
        100,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let kind_filter = q.kind.as_deref().unwrap_or("all");
    let mut out: Vec<JsonValue> = Vec::new();
    let mut violation_count: u64 = 0;

    for s in rows {
        let schematic: WyckoffSchematic = serde_json::from_value(
            JsonValue::String(s.schematic.clone()),
        )
        .unwrap_or(WyckoffSchematic::Accumulation);
        let structure_phase: WyckoffPhase = serde_json::from_value(
            JsonValue::String(s.current_phase.clone()),
        )
        .unwrap_or(WyckoffPhase::A);

        let events_arr = match s.events_json.as_array() {
            Some(a) => a.clone(),
            None => continue,
        };

        // Sort by bar_index (or time_ms if present) so prior_max_phase
        // is meaningful for regression detection.
        let mut sorted = events_arr;
        sorted.sort_by_key(|v| {
            v.get("time_ms")
                .and_then(|t| t.as_i64())
                .unwrap_or_else(|| {
                    v.get("bar_index").and_then(|b| b.as_i64()).unwrap_or(0)
                })
        });

        let mut prior_max_phase = WyckoffPhase::A;

        for ev_json in &sorted {
            let event: WyckoffEvent = match ev_json
                .get("event")
                .and_then(|v| serde_json::from_value::<WyckoffEvent>(v.clone()).ok())
            {
                Some(e) => e,
                None => continue,
            };
            let bar_index = ev_json.get("bar_index").and_then(|v| v.as_u64());
            let price = ev_json.get("price").and_then(|v| v.as_f64());
            let score = ev_json.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let time_ms = ev_json.get("time_ms").and_then(|v| v.as_i64());

            let violation =
                validate_event_placement(event, structure_phase, schematic, prior_max_phase);
            let is_violation = violation.is_some();
            if is_violation {
                violation_count += 1;
            }

            let include = match kind_filter {
                "violations" => is_violation,
                "valid" => !is_violation,
                _ => true,
            };
            if !include {
                // still advance prior_max_phase so downstream regression
                // detection stays accurate.
                if event.phase() > prior_max_phase {
                    prior_max_phase = event.phase();
                }
                continue;
            }

            out.push(json!({
                "struct_id": s.id,
                "symbol": s.symbol,
                "interval": s.interval,
                "schematic": s.schematic,
                "structure_phase": s.current_phase,
                "event_code": event.as_str(),
                "full_name": event.full_name(),
                "phase": event.phase().as_str(),
                "family": event.family().as_str(),
                "bar_index": bar_index,
                "price": price,
                "score": score,
                "time_ms": time_ms,
                "violation": violation.as_ref().map(|v| json!({
                    "kind": v.kind,
                    "reason": v.reason,
                })),
            }));

            if event.phase() > prior_max_phase {
                prior_max_phase = event.phase();
            }
            if out.len() as i64 >= limit {
                break;
            }
        }

        if out.len() as i64 >= limit {
            break;
        }
    }

    Ok(Json(json!({
        "generated_at": chrono::Utc::now(),
        "count": out.len(),
        "violation_count": violation_count,
        "events": out,
    })))
}

/// Wyckoff-scoped setup feed. Thin wrapper around
/// `list_v2_setups_filtered` with `alt_type LIKE 'wyckoff_%'` pre-applied.
/// Default `state` filter: armed+active. Pass `state=closed` for history.
async fn get_wyckoff_setups(
    State(st): State<SharedState>,
    Query(q): Query<WyckoffSetupsQuery>,
) -> Result<Json<JsonValue>, ApiError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 2_000);
    let states = q
        .state
        .as_deref()
        .map(|s| vec![s.to_string()])
        .unwrap_or_default();
    let filter = SetupFilter {
        limit,
        venue_class: q.venue,
        states,
        profile: q.profile,
        alt_type_like: Some("wyckoff_%".to_string()),
        symbol: q.symbol,
        timeframe: q.timeframe,
        mode: q.mode,
    };
    let rows = list_v2_setups_filtered(&st.pool, &filter).await?;
    Ok(Json(json!({
        "generated_at": chrono::Utc::now(),
        "count": rows.len(),
        "entries": rows,
    })))
}

async fn get_active(
    State(st): State<SharedState>,
    Query(q): Query<ActiveQuery>,
) -> Result<Json<JsonValue>, ApiError> {
    let rows = list_active_wyckoff_structures(
        &st.pool,
        q.symbol.as_deref(),
        q.interval.as_deref(),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "structures": rows })))
}

/// Cross-symbol recent feed. `status=active|completed|failed|all`
/// (default `all`). Ordered by lifecycle timestamp DESC so the most
/// recently closed structures surface first.
async fn get_recent(
    State(st): State<SharedState>,
    Query(q): Query<RecentQuery>,
) -> Result<Json<JsonValue>, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows = list_recent_wyckoff_structures(
        &st.pool,
        q.status.as_deref(),
        q.exchange.as_deref(),
        q.symbol.as_deref(),
        q.interval.as_deref(),
        limit,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "structures": rows })))
}

/// Phase counts grouped per (exchange, symbol, interval). Backs the
/// "phases grouped by timeframe" summary in the GUI — aggregates the
/// entire history since the first stored structure.
async fn get_phase_groups(
    State(st): State<SharedState>,
    Query(q): Query<PhaseGroupsQuery>,
) -> Result<Json<JsonValue>, ApiError> {
    let rows = list_phase_groups_by_timeframe(
        &st.pool,
        q.exchange.as_deref(),
        q.symbol.as_deref(),
        q.interval.as_deref(),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "groups": rows })))
}

async fn get_structure(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<JsonValue>, ApiError> {
    let row = get_wyckoff_structure(&st.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    match row {
        Some(r) => Ok(Json(json!(r))),
        None => Err(ApiError::not_found("structure not found")),
    }
}

async fn get_history(
    State(st): State<SharedState>,
    Path(symbol): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<JsonValue>, ApiError> {
    let limit = q.limit.unwrap_or(20).clamp(1, 200);
    let rows = list_wyckoff_history(&st.pool, &symbol, limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "history": rows })))
}

/// Chart overlay: returns range box, creek/ice, events with timestamps.
async fn get_overlay(
    State(st): State<SharedState>,
    Path((symbol, interval)): Path<(String, String)>,
) -> Result<Json<JsonValue>, ApiError> {
    let structure = find_active_wyckoff_structure(&st.pool, &symbol, &interval)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let s = match structure {
        Some(s) => s,
        None => return Ok(Json(json!({ "overlay": null }))),
    };
    Ok(Json(json!({
        "overlay": {
            "id": s.id,
            "schematic": s.schematic,
            "phase": s.current_phase,
            "confidence": s.confidence,
            "range": {
                "top": s.range_top,
                "bottom": s.range_bottom,
            },
            "creek": s.creek_level,
            "ice": s.ice_level,
            "slope_deg": s.slope_deg,
            "events": s.events_json,
            "started_at": s.started_at,
        }
    })))
}
