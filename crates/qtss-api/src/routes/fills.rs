//! Live fills (`exchange_fills`) — user-scoped recent list.

use axum::extract::{Extension, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use qtss_storage::ExchangeFillRow;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct ListFillsQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

pub fn fills_router() -> Router<SharedState> {
    Router::new().route("/fills", get(list_my_fills))
}

async fn list_my_fills(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<ListFillsQuery>,
) -> Result<Json<Vec<ExchangeFillRow>>, ApiError> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    let lim = q.limit.clamp(1, 500);
    let rows = st.exchange_fills.list_recent_for_user(user_id, lim).await?;
    Ok(Json(rows))
}

