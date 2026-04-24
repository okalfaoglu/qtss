//! `GET /v2/fees` — current commission schedule snapshot.
//!
//! Small read-only endpoint so the GUI (Setups drawer, position-sizing
//! widgets, BE ratchet panel) can render commission-aware numbers
//! without re-hardcoding the schedule. Values are basis points as
//! written by the `setup.commission.*` rows in `system_config`; one
//! `bps` = 0.01%. The round-trip cost of a taker position is thus
//! `taker_bps * 2 / 10000` expressed as a fraction of notional.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sqlx::Row;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Default, Serialize)]
pub struct FeeSchedule {
    /// e.g. `"binance_futures"` — matches the suffix used in system_config.
    pub venue: &'static str,
    pub maker_bps: Option<f64>,
    pub taker_bps: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct FeeSnapshot {
    pub schedules: Vec<FeeSchedule>,
    /// Convenience fallback when the caller has not identified a venue
    /// yet (e.g. a setup row with no live_positions trail). Equals
    /// `binance_futures.taker_bps` by default.
    pub default_taker_bps: Option<f64>,
}

pub fn v2_fees_router() -> Router<SharedState> {
    Router::new().route("/v2/fees", get(get_fees))
}

async fn get_fees(State(st): State<SharedState>) -> Result<Json<FeeSnapshot>, ApiError> {
    let rows = sqlx::query(
        r#"SELECT config_key, value
             FROM system_config
            WHERE module = 'setup'
              AND config_key LIKE 'commission.%.%_bps'"#,
    )
    .fetch_all(&st.pool)
    .await?;

    let mut futures = FeeSchedule {
        venue: "binance_futures",
        ..Default::default()
    };
    let mut spot = FeeSchedule {
        venue: "binance_spot",
        ..Default::default()
    };

    for r in rows {
        let key: String = r.try_get("config_key").unwrap_or_default();
        let val: serde_json::Value = r.try_get("value").unwrap_or(serde_json::Value::Null);
        let bps = match &val {
            // Historical seeds stored the number directly (e.g. `5.0`)
            // rather than wrapping it in `{"value": 5}`, so accept both.
            serde_json::Value::Number(n) => n.as_f64(),
            other => other.get("value").and_then(|v| v.as_f64()),
        };
        let Some(bps) = bps else {
            continue;
        };
        // Key shape: commission.<venue>.<liquidity>_bps
        match key.as_str() {
            "commission.binance_futures.maker_bps" => futures.maker_bps = Some(bps),
            "commission.binance_futures.taker_bps" => futures.taker_bps = Some(bps),
            "commission.binance_spot.maker_bps" => spot.maker_bps = Some(bps),
            "commission.binance_spot.taker_bps" => spot.taker_bps = Some(bps),
            _ => {}
        }
    }

    let default_taker_bps = futures.taker_bps.or(spot.taker_bps);
    Ok(Json(FeeSnapshot {
        schedules: vec![futures, spot],
        default_taker_bps,
    }))
}
