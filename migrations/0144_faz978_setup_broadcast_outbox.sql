-- Faz 9.7.8 — Setup broadcast outbox.
--
-- When a new row lands in qtss_v2_setups, a sibling row is enqueued
-- here by the setup engine. A dedicated worker loop claims pending
-- rows, builds a PublicCard (with AI brief populated from
-- qtss_ml_predictions + raw_meta.ai), dispatches to Telegram, and
-- enqueues to x_outbox.
--
-- Idempotency: UNIQUE(setup_id). Safe to retry enqueue.

CREATE TABLE IF NOT EXISTS setup_broadcast_outbox (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    setup_id          UUID NOT NULL UNIQUE,
    status            TEXT NOT NULL DEFAULT 'pending'
                          CHECK (status IN ('pending','claimed','sent','failed')),
    attempts          INTEGER NOT NULL DEFAULT 0,
    last_error        TEXT,
    telegram_sent_at  TIMESTAMPTZ,
    x_enqueued_at     TIMESTAMPTZ,
    claimed_at        TIMESTAMPTZ,
    sent_at           TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_setup_bcast_outbox_pending
    ON setup_broadcast_outbox (created_at)
    WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_setup_bcast_outbox_status
    ON setup_broadcast_outbox (status, updated_at DESC);

COMMENT ON TABLE setup_broadcast_outbox IS
    'Faz 9.7.8 outbox: new-setup PublicCard dispatch to Telegram + X.';

-- Config keys — publisher loop gate + cadence.
SELECT _qtss_register_key(
    'setup_publisher.enabled', 'notify', 'setup_publisher',
    'bool', 'false'::jsonb, '',
    'Master switch for the new-setup public broadcast loop (Telegram + X outbox).',
    'bool', true, 'high', ARRAY['notify','faz978','setup_publisher']
);

SELECT _qtss_register_key(
    'setup_publisher.tick_secs', 'notify', 'setup_publisher',
    'int', '10'::jsonb, 's',
    'Polling cadence of the setup publisher loop (seconds).',
    'number', false, 'normal', ARRAY['notify','faz978','setup_publisher']
);

SELECT _qtss_register_key(
    'setup_publisher.batch_limit', 'notify', 'setup_publisher',
    'int', '20'::jsonb, '',
    'Max outbox rows claimed per tick.',
    'number', false, 'normal', ARRAY['notify','faz978','setup_publisher']
);

SELECT _qtss_register_key(
    'setup_publisher.max_attempts', 'notify', 'setup_publisher',
    'int', '5'::jsonb, '',
    'After this many consecutive failures a row is marked terminally failed.',
    'number', false, 'normal', ARRAY['notify','faz978','setup_publisher']
);
