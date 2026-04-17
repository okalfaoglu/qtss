-- Faz 9.7.0 — X (Twitter) outbox queue.
--
-- Mirror of notify_outbox but for X posts. Kept separate because:
--   - 280-char rendering is distinct from Telegram HTML
--   - X has strict daily/monthly rate limits requiring its own cadence
--   - Image attachments go through a different upload flow (media_id)
--   - Delivery receipts (tweet_id, permalink) are X-specific

CREATE TABLE IF NOT EXISTS x_outbox (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Source linkage (either lifecycle event or a raw setup broadcast).
    setup_id        UUID REFERENCES qtss_v2_setups(id) ON DELETE SET NULL,
    lifecycle_event_id UUID REFERENCES qtss_setup_lifecycle_events(id) ON DELETE SET NULL,
    -- Payload.
    event_key       TEXT NOT NULL,        -- e.g. "setup.opened", "lifecycle.tp_final"
    body            TEXT NOT NULL,        -- the 280-char post text
    image_path      TEXT,                 -- local filesystem path or URL to chart PNG
    -- Queue state.
    status          TEXT NOT NULL DEFAULT 'pending',   -- pending|sending|sent|failed|skipped
    attempt_count   SMALLINT NOT NULL DEFAULT 0,
    last_error      TEXT,
    -- Delivery results.
    tweet_id        TEXT,
    permalink       TEXT,
    sent_at         TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT x_outbox_status_chk
        CHECK (status IN ('pending','sending','sent','failed','skipped'))
);

CREATE INDEX IF NOT EXISTS idx_x_outbox_status_created
    ON x_outbox (status, created_at)
    WHERE status IN ('pending','sending');
CREATE INDEX IF NOT EXISTS idx_x_outbox_setup
    ON x_outbox (setup_id);
CREATE INDEX IF NOT EXISTS idx_x_outbox_sent_at
    ON x_outbox (sent_at DESC)
    WHERE status = 'sent';

-- Daily cap enforcement helper: how many posts sent today (UTC).
-- Query pattern: SELECT COUNT(*) FROM x_outbox WHERE status='sent' AND sent_at >= date_trunc('day', now() at time zone 'utc');

-- X API credentials config keys (API key/secret stored here because
-- they're required for the publisher loop to function; secrets class
-- is flagged so GUI masks them).
SELECT _qtss_register_key(
    'x.api.bearer_token', 'notify', 'x',
    'string', '""'::jsonb, '',
    'X (Twitter) API v2 Bearer token for posting.',
    'text', true, 'high', ARRAY['notify','faz97','x','secret']
);
SELECT _qtss_register_key(
    'x.api.oauth1_consumer_key', 'notify', 'x',
    'string', '""'::jsonb, '',
    'X OAuth1 consumer key (needed for media upload).',
    'text', true, 'high', ARRAY['notify','faz97','x','secret']
);
SELECT _qtss_register_key(
    'x.api.oauth1_consumer_secret', 'notify', 'x',
    'string', '""'::jsonb, '',
    'X OAuth1 consumer secret.',
    'text', true, 'high', ARRAY['notify','faz97','x','secret']
);
SELECT _qtss_register_key(
    'x.api.oauth1_access_token', 'notify', 'x',
    'string', '""'::jsonb, '',
    'X OAuth1 access token.',
    'text', true, 'high', ARRAY['notify','faz97','x','secret']
);
SELECT _qtss_register_key(
    'x.api.oauth1_access_secret', 'notify', 'x',
    'string', '""'::jsonb, '',
    'X OAuth1 access secret.',
    'text', true, 'high', ARRAY['notify','faz97','x','secret']
);
SELECT _qtss_register_key(
    'x.publisher.tick_secs', 'notify', 'x',
    'int', '10'::jsonb, '',
    'Seconds between x_outbox publisher loop ticks.',
    'number', true, 'normal', ARRAY['notify','faz97','x']
);
