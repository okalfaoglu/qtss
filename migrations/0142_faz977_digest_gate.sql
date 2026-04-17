-- Faz 9.7.7 — Daily-digest scheduler gate.
-- `last_digest_sent_utc` stamps the most recent successful digest
-- delivery, so the scan loop can skip users who already received
-- theirs for the current local day.

ALTER TABLE notify_delivery_prefs
    ADD COLUMN IF NOT EXISTS last_digest_sent_utc TIMESTAMPTZ;

COMMENT ON COLUMN notify_delivery_prefs.last_digest_sent_utc IS
    'UTC timestamp of the most recent successful daily digest delivery for this user. NULL = never sent.';

CREATE INDEX IF NOT EXISTS idx_notify_prefs_last_digest
    ON notify_delivery_prefs (last_digest_sent_utc);

-- Digest scheduling config.
SELECT _qtss_register_key(
    'digest.enabled', 'notify', 'digest',
    'bool', 'false'::jsonb, '',
    'Master switch for the per-user daily digest loop.',
    'bool', true, 'normal', ARRAY['notify','faz97','digest']
);

SELECT _qtss_register_key(
    'digest.scan_tick_secs', 'notify', 'digest',
    'int', '600'::jsonb, 's',
    'How often the digest loop scans for due users (seconds).',
    'number', false, 'normal', ARRAY['notify','faz97','digest']
);

SELECT _qtss_register_key(
    'digest.local_hour', 'notify', 'digest',
    'int', '18'::jsonb, 'h',
    'Local-time hour (0..23) at which each user receives their daily digest.',
    'number', false, 'normal', ARRAY['notify','faz97','digest']
);

SELECT _qtss_register_key(
    'digest.window_hours', 'notify', 'digest',
    'int', '24'::jsonb, 'h',
    'Lookback window in hours for digest aggregation.',
    'number', false, 'normal', ARRAY['notify','faz97','digest']
);
