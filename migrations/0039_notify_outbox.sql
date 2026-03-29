-- Async notification outbox (beyond direct `qtss-notify` calls) — dev guide §9.1 item 7.

CREATE TABLE notify_outbox (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID REFERENCES organizations (id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    channels JSONB NOT NULL DEFAULT '["webhook"]'::jsonb,
    status TEXT NOT NULL DEFAULT 'pending',
    attempt_count INT NOT NULL DEFAULT 0,
    last_error TEXT,
    sent_at TIMESTAMPTZ,
    delivery_detail JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT notify_outbox_status_chk CHECK (
        status IN (
            'pending',
            'sending',
            'sent',
            'failed'
        )
    )
);

CREATE INDEX idx_notify_outbox_pending_created ON notify_outbox (created_at ASC)
WHERE
    status = 'pending';
