-- HTTP mutasyonları için denetim izi (isteğe bağlı tetik; bakınız QTSS_AUDIT_HTTP).

CREATE TABLE audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    request_id TEXT,
    user_id UUID REFERENCES users (id) ON DELETE SET NULL,
    org_id UUID REFERENCES organizations (id) ON DELETE SET NULL,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    status_code SMALLINT NOT NULL,
    roles TEXT[] NOT NULL DEFAULT '{}'
);

CREATE INDEX idx_audit_log_created ON audit_log (created_at DESC);
CREATE INDEX idx_audit_log_user ON audit_log (user_id, created_at DESC);
