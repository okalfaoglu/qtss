//! Public supported locales catalog (FAZ 9.2) — align with `web/src/locales/supportedLocales.ts`.

use axum::{routing::get, Json, Router};
use serde::Serialize;

use crate::state::SharedState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocaleEntry {
    code: &'static str,
    native_name: &'static str,
    dir: &'static str,
}

#[derive(Serialize)]
struct SupportedLocalesBody {
    locales: Vec<LocaleEntry>,
}

async fn get_supported_locales() -> Json<SupportedLocalesBody> {
    Json(SupportedLocalesBody {
        locales: vec![
            LocaleEntry {
                code: "en",
                native_name: "English",
                dir: "ltr",
            },
            LocaleEntry {
                code: "tr",
                native_name: "Türkçe",
                dir: "ltr",
            },
        ],
    })
}

/// Merged under `/api/v1` before the JWT-protected router — no Bearer required.
pub fn public_locales_routes() -> Router<SharedState> {
    Router::new().route("/locales", get(get_supported_locales))
}
