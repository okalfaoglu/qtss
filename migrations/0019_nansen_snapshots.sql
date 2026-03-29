-- Nansen token screener: worker `upsert_nansen_snapshot` — `snapshot_kind` başına tek satır.

CREATE TABLE nansen_snapshots (
    snapshot_kind TEXT PRIMARY KEY,
    request_json JSONB NOT NULL,
    response_json JSONB,
    meta_json JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);
