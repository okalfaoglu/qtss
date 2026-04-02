-- Harici HTTP kaynak tanımları; yanıtlar `data_snapshots` (`external_fetch_engine`, ops API).
-- Idempotent: `0013_worker_analytics_schema.sql` may already create `external_data_sources`.
-- Ensures `created_at` and the partial index from the original 0021 migration.

CREATE TABLE IF NOT EXISTS external_data_sources (
    key TEXT PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    headers_json JSONB NOT NULL DEFAULT '{}',
    body_json JSONB,
    tick_secs INTEGER NOT NULL DEFAULT 30,
    description TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE external_data_sources
    ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT now();

CREATE INDEX IF NOT EXISTS idx_external_data_sources_enabled ON external_data_sources (enabled) WHERE enabled = true;
