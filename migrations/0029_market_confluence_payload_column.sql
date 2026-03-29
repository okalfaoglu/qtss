-- Phase B — tam confluence payload kopyası (geçmiş / UI; analysis_snapshots ile çift yazım).

ALTER TABLE market_confluence_snapshots
    ADD COLUMN IF NOT EXISTS confluence_payload_json JSONB;

COMMENT ON COLUMN market_confluence_snapshots.confluence_payload_json IS 'Full confluence engine payload (schema_version 2+) at compute time.';
