-- Birleşik anlık görüntü: `source_key` başına tek satır (Nansen + generic HTTP + confluence okuma).

CREATE TABLE data_snapshots (
    source_key TEXT PRIMARY KEY,
    request_json JSONB NOT NULL,
    response_json JSONB,
    meta_json JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);

CREATE INDEX idx_data_snapshots_computed ON data_snapshots (computed_at DESC);
