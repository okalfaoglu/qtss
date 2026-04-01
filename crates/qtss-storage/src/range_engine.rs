//! `app_config.key = range_engine` — Trading Range / sinyal paneli worker ayarları ve web’den tetiklenen yenileme bayrağı.

use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::StorageError;
use crate::AppConfigEntry;
use sqlx::PgPool;

pub const RANGE_ENGINE_APP_CONFIG_KEY: &str = "range_engine";

/// Varsayılan iskelet: eksik alanlar `fetch_range_engine_json` ile tamamlanır.
pub fn default_range_engine_json() -> Value {
    json!({
        "trading_range_params": {
            "lookback": null,
            "atr_period": null,
            "atr_sma_period": null,
            "require_range_regime": null
        },
        "execution_gates": {
            "allow_long_open": true,
            "allow_short_open": true,
            "allow_all_closes": true
        },
        "worker": {
            "refresh_requested": false
        }
    })
}

/// `patch` içindeki nesneleri `base` üzerine derin birleştirir (yaprakları patch yener).
pub fn merge_json_deep(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(bm), Value::Object(pm)) => {
            for (k, pv) in pm {
                match bm.get_mut(k) {
                    Some(bv) => merge_json_deep(bv, pv),
                    None => {
                        bm.insert(k.clone(), pv.clone());
                    }
                }
            }
        }
        (b, p) => {
            *b = p.clone();
        }
    }
}

/// DB + varsayılan birleşik belge (worker ve API ortak).
pub async fn fetch_range_engine_json(pool: &PgPool) -> Result<Value, StorageError> {
    let mut doc = default_range_engine_json();
    if let Some(stored) = crate::AppConfigRepository::get_value_json(pool, RANGE_ENGINE_APP_CONFIG_KEY).await? {
        merge_json_deep(&mut doc, &stored);
    }
    Ok(doc)
}

pub async fn upsert_range_engine_json(
    pool: &PgPool,
    value: Value,
    updated_by_user_id: Option<Uuid>,
) -> Result<AppConfigEntry, StorageError> {
    let repo = crate::AppConfigRepository::new(pool.clone());
    repo
        .upsert(
            RANGE_ENGINE_APP_CONFIG_KEY,
            value,
            Some("Trading range + signal panel: worker params, execution gates, refresh flag"),
            updated_by_user_id,
        )
        .await
}

/// Worker başarılı tur sonrası `worker.refresh_requested` sıfırlar.
pub async fn clear_refresh_requested(pool: &PgPool) -> Result<(), StorageError> {
    let mut doc = fetch_range_engine_json(pool).await?;
    if let Some(w) = doc.get_mut("worker").and_then(|x| x.as_object_mut()) {
        w.insert("refresh_requested".into(), json!(false));
    }
    upsert_range_engine_json(pool, doc, None).await?;
    Ok(())
}
