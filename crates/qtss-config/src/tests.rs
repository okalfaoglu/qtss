//! Unit tests for `qtss-config`. These cover scope resolution and the
//! `MemoryConfigStore`. Postgres integration is exercised via the workspace
//! integration test harness (added in a later PR).

use crate::scope::{ResolveCtx, Scope, ScopeType};
use crate::store::{ConfigStore, MemoryConfigStore};
use crate::types::{ConfigSchemaRow, SetOptions};
use serde_json::json;

fn schema(key: &str, default: serde_json::Value) -> ConfigSchemaRow {
    ConfigSchemaRow {
        key: key.to_string(),
        category: "test".into(),
        subcategory: None,
        value_type: "float".into(),
        json_schema: json!({}),
        default_value: default,
        unit: None,
        description: "test key".into(),
        ui_widget: None,
        requires_restart: false,
        is_secret_ref: false,
        sensitivity: "normal".into(),
        deprecated_at: None,
        introduced_in: Some("v2.0".into()),
        tags: vec![],
    }
}

#[tokio::test]
async fn falls_back_to_schema_default_when_no_overrides() {
    let store = MemoryConfigStore::new();
    store.register_schema(schema("risk.account.max_drawdown_pct", json!(10.0)));

    let v: f64 = store
        .get("risk.account.max_drawdown_pct", &ResolveCtx::default())
        .await
        .unwrap();
    assert_eq!(v, 10.0);
}

#[tokio::test]
async fn instrument_scope_beats_venue_scope() {
    let store = MemoryConfigStore::new();
    store.register_schema(schema("risk.position.default_risk_pct", json!(0.5)));

    store.put(
        "risk.position.default_risk_pct",
        &Scope::new(ScopeType::Venue, "binance"),
        json!(0.7),
    );
    store.put(
        "risk.position.default_risk_pct",
        &Scope::new(ScopeType::Instrument, "BTCUSDT"),
        json!(0.3),
    );

    let ctx = ResolveCtx::default()
        .with_venue("binance")
        .with_instrument("BTCUSDT");

    let v: f64 = store
        .get("risk.position.default_risk_pct", &ctx)
        .await
        .unwrap();
    assert_eq!(v, 0.3, "instrument override must beat venue override");
}

#[tokio::test]
async fn venue_scope_used_when_instrument_unset() {
    let store = MemoryConfigStore::new();
    store.register_schema(schema("risk.position.default_risk_pct", json!(0.5)));

    store.put(
        "risk.position.default_risk_pct",
        &Scope::new(ScopeType::Venue, "binance"),
        json!(0.7),
    );

    let ctx = ResolveCtx::default().with_venue("binance");
    let v: f64 = store
        .get("risk.position.default_risk_pct", &ctx)
        .await
        .unwrap();
    assert_eq!(v, 0.7);
}

#[tokio::test]
async fn unknown_key_returns_not_found() {
    let store = MemoryConfigStore::new();
    let result: crate::ConfigResult<f64> = store
        .get("does.not.exist", &ResolveCtx::default())
        .await;
    assert!(matches!(result, Err(crate::ConfigError::NotFound(_))));
}

#[tokio::test]
async fn set_requires_reason() {
    let store = MemoryConfigStore::new();
    store.register_schema(schema("risk.account.max_drawdown_pct", json!(10.0)));

    let err = store
        .set_json(
            "risk.account.max_drawdown_pct",
            &Scope::global(),
            json!(8.0),
            None,
            "   ",
            SetOptions::default(),
        )
        .await;
    assert!(matches!(err, Err(crate::ConfigError::MissingReason)));
}

#[tokio::test]
async fn set_then_get_round_trip() {
    let store = MemoryConfigStore::new();
    store.register_schema(schema("risk.account.max_drawdown_pct", json!(10.0)));

    store
        .set_json(
            "risk.account.max_drawdown_pct",
            &Scope::global(),
            json!(7.5),
            None,
            "tighten DD for cautious phase",
            SetOptions::default(),
        )
        .await
        .unwrap();

    let v: f64 = store
        .get("risk.account.max_drawdown_pct", &ResolveCtx::default())
        .await
        .unwrap();
    assert_eq!(v, 7.5);
}
