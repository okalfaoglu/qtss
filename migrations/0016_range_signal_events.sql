-- Range sinyal olayları (F1): worker `insert_range_signal_event` — aynı (hedef, tür, bar) tekrar yazılmaz.
-- `0013_worker_analytics_schema.sql` bu tabloyu da oluşturabildiği için IF NOT EXISTS.

CREATE TABLE IF NOT EXISTS range_signal_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    event_kind TEXT NOT NULL,
    bar_open_time TIMESTAMPTZ NOT NULL,
    reference_price DOUBLE PRECISION,
    source TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (engine_symbol_id, event_kind, bar_open_time)
);

CREATE INDEX IF NOT EXISTS idx_range_signal_events_engine_time
  ON range_signal_events (engine_symbol_id, bar_open_time DESC);
