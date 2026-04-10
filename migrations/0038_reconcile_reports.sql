-- 0038 — reconcile_reports: persist v2 ReconcileReport snapshots.
-- Workers write after each periodic reconciliation run.

CREATE TABLE IF NOT EXISTS reconcile_reports (
    id              BIGSERIAL PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    venue           TEXT NOT NULL,
    overall         TEXT NOT NULL CHECK (overall IN ('none','soft','drift','critical')),
    position_drifts JSONB NOT NULL DEFAULT '[]'::jsonb,
    order_drifts    JSONB NOT NULL DEFAULT '[]'::jsonb,
    position_count  INT NOT NULL DEFAULT 0,
    order_count     INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_reconcile_reports_user_venue
    ON reconcile_reports (user_id, venue, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_reconcile_reports_severity
    ON reconcile_reports (overall, created_at DESC)
    WHERE overall <> 'none';
