//! `WriterTask` — dispatch trait for a single pattern-family writer.

use async_trait::async_trait;
use sqlx::PgPool;

#[async_trait]
pub trait WriterTask: Send + Sync {
    /// Short identifier used in tracing (e.g. `"pivot"`, `"elliott"`,
    /// `"harmonic"`). Also the default `system_config` module name the
    /// writer reads its per-family config from.
    fn family_name(&self) -> &'static str;

    /// Per-family kill switch. Default impl reads
    /// `system_config.<family>.enabled -> { "enabled": bool }` with
    /// `true` as the fallback — matches the legacy loops' behaviour.
    async fn is_enabled(&self, pool: &PgPool) -> bool {
        let key = self.family_name();
        let row = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT value FROM system_config WHERE module = $1 AND config_key = 'enabled'",
        )
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        let Some((val,)) = row else { return true; };
        val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
    }

    /// Run one full sweep over all enabled engine_symbols and return
    /// aggregate stats for logging.
    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats>;
}

#[derive(Default, Debug, Clone, Copy)]
pub struct RunStats {
    pub series_processed: usize,
    pub rows_upserted: usize,
}

impl RunStats {
    pub fn add(&mut self, other: RunStats) {
        self.series_processed += other.series_processed;
        self.rows_upserted += other.rows_upserted;
    }
}
