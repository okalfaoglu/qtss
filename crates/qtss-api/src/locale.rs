//! `Accept-Language` / `?locale=` negotiation for API responses (FAZ 9.2).

use axum::extract::Request;
use axum::http::header::ACCEPT_LANGUAGE;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

/// Resolved locale for the request (`Accept-Language` / `?locale=`). Handlers read via [`Self::as_str`].
#[derive(Clone, Debug)]
pub struct NegotiatedLocale(String);

impl NegotiatedLocale {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn locale_from_accept_language(raw: &str) -> String {
    for part in raw.split(',') {
        let tag = part.split(';').next().unwrap_or("").trim().to_lowercase();
        if tag.starts_with("tr") {
            return "tr".into();
        }
        if tag.starts_with("en") {
            return "en".into();
        }
    }
    "en".into()
}

fn locale_from_query(query: Option<&str>) -> Option<String> {
    let q = query?;
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        let k = it.next().unwrap_or("").trim();
        let v = it.next().unwrap_or("").trim();
        if k != "locale" {
            continue;
        }
        let t = v.to_lowercase();
        if t.starts_with("tr") {
            return Some("tr".into());
        }
        if t.starts_with("en") {
            return Some("en".into());
        }
    }
    None
}

pub async fn locale_middleware(mut req: Request, next: Next) -> Response {
    let from_query = locale_from_query(req.uri().query());
    let from_accept = req
        .headers()
        .get(ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .map(locale_from_accept_language)
        .unwrap_or_else(|| "en".into());
    let loc = from_query.unwrap_or(from_accept);
    let negotiated = NegotiatedLocale(loc);
    let response_locale = negotiated.as_str().to_string();
    req.extensions_mut().insert(negotiated);
    let mut res = next.run(req).await;
    if let Ok(hv) = HeaderValue::from_str(&response_locale) {
        res.headers_mut().insert(
            HeaderName::from_static("x-qtss-negotiated-locale"),
            hv,
        );
    }
    res
}
