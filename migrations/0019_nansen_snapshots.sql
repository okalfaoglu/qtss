-- Nansen token screener: worker `upsert_nansen_snapshot` — `snapshot_kind` başına tek satır.
-- Idempotent: `0013_worker_analytics_schema.sql` may already create this table.

CREATE TABLE IF NOT EXISTS nansen_snapshots (
    snapshot_kind TEXT PRIMARY KEY,
    request_json JSONB NOT NULL,
    response_json JSONB,
    meta_json JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);
