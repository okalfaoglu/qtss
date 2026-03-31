-- Enrich notify_outbox: filterable metadata fields (event_key / instrument).

ALTER TABLE notify_outbox
ADD COLUMN event_key TEXT,
ADD COLUMN severity TEXT NOT NULL DEFAULT 'info',
ADD COLUMN exchange TEXT,
ADD COLUMN segment TEXT,
ADD COLUMN symbol TEXT;

ALTER TABLE notify_outbox
ADD CONSTRAINT notify_outbox_severity_chk CHECK (severity IN ('info', 'warn', 'error'));

CREATE INDEX idx_notify_outbox_org_created ON notify_outbox (org_id, created_at DESC);
CREATE INDEX idx_notify_outbox_event_key ON notify_outbox (event_key);
CREATE INDEX idx_notify_outbox_instrument ON notify_outbox (exchange, segment, symbol);

