//! Shared helpers for reading `engine_symbols` — the single source of
//! truth for "which series should any writer touch this tick".

use sqlx::{PgPool, Row};

#[derive(Debug, Clone)]
pub struct EngineSymbol {
    pub id: sqlx::types::Uuid,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
}

pub async fn list_enabled(pool: &PgPool) -> anyhow::Result<Vec<EngineSymbol>> {
    let rows = sqlx::query(
        r#"SELECT id, exchange, segment, symbol, "interval"
             FROM engine_symbols
            WHERE enabled = true
            ORDER BY exchange, segment, symbol, "interval""#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| EngineSymbol {
            id: r.get("id"),
            exchange: r.get("exchange"),
            segment: r.get("segment"),
            symbol: r.get("symbol"),
            interval: r.get("interval"),
        })
        .collect())
}

/// Pull the Z-slot → zigzag-length ladder from `system_config.zigzag.slot_N`.
/// Defaults: `[3, 5, 8, 13, 21]` — Fibonacci ladder shared by pivot and
/// elliott writers. `load_slot_lengths` here is the single reader — the
/// writers delegate so operator tweaks in the Config Editor flow through
/// both simultaneously.
pub async fn load_slot_lengths(pool: &PgPool) -> [u32; 5] {
    let defaults: [u32; 5] = [3, 5, 8, 13, 21];
    let mut out = defaults;
    for i in 0..5usize {
        let key = format!("slot_{i}");
        if let Ok(Some(row)) = sqlx::query(
            "SELECT value FROM system_config WHERE module = 'zigzag' AND config_key = $1",
        )
        .bind(&key)
        .fetch_optional(pool)
        .await
        {
            let val: serde_json::Value =
                row.try_get("value").unwrap_or(serde_json::Value::Null);
            if let Some(len) = val.get("length").and_then(|v| v.as_u64()) {
                out[i] = (len.max(1)) as u32;
            }
        }
    }
    out
}
