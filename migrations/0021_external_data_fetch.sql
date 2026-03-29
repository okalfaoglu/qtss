-- Harici HTTP kaynak tanımları; yanıtlar `data_snapshots` (`external_fetch_engine`, ops API).

CREATE TABLE external_data_sources (
    key TEXT PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT true,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    headers_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    body_json JSONB,
    tick_secs INT NOT NULL DEFAULT 300,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_external_data_sources_enabled ON external_data_sources (enabled) WHERE enabled = true;
