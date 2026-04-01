-- Idempotent paper execution for range_signal_events (worker optional loop).
CREATE TABLE IF NOT EXISTS range_signal_paper_executions (
    range_signal_event_id UUID PRIMARY KEY REFERENCES range_signal_events (id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    client_order_id UUID,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_range_signal_paper_executions_status_created
    ON range_signal_paper_executions (status, created_at DESC);
