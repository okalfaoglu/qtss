-- Birleşik anlık görüntü: `source_key` başına tek satır (Nansen + generic HTTP + confluence okuma).
-- Idempotent: `0013_worker_analytics_schema.sql` may already create `data_snapshots`.

CREATE TABLE IF NOT EXISTS data_snapshots (
    source_key TEXT PRIMARY KEY,
    request_json JSONB NOT NULL,
    response_json JSONB,
    meta_json JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);

CREATE INDEX IF NOT EXISTS idx_data_snapshots_computed ON data_snapshots (computed_at DESC);
