-- Global Nansen API snapshots (not tied to engine_symbols / analysis_snapshots).

CREATE TABLE nansen_snapshots (
    snapshot_kind TEXT PRIMARY KEY,
    request_json JSONB NOT NULL,
    response_json JSONB,
    meta_json JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);

CREATE INDEX idx_nansen_snapshots_computed ON nansen_snapshots (computed_at DESC);
