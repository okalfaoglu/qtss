-- AI / policy human approval queue (dev guide §9.1 item 6).

CREATE TABLE ai_approval_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    requester_user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending',
    kind TEXT NOT NULL DEFAULT 'generic',
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    model_hint TEXT,
    admin_note TEXT,
    decided_by_user_id UUID REFERENCES users (id) ON DELETE SET NULL,
    decided_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT ai_approval_requests_status_chk CHECK (
        status IN (
            'pending',
            'approved',
            'rejected',
            'cancelled'
        )
    )
);

CREATE INDEX idx_ai_approval_org_status_created ON ai_approval_requests (org_id, status, created_at DESC);
