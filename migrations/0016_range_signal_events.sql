-- F1: Range motorundan türetilen long/short giriş-çıkış olayları (şartname SPEC_EXECUTION_RANGE_SIGNALS_UI.md).

CREATE TABLE range_signal_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    event_kind TEXT NOT NULL CHECK (
        event_kind IN (
            'long_entry',
            'long_exit',
            'short_entry',
            'short_exit'
        )
    ),
    bar_open_time TIMESTAMPTZ NOT NULL,
    reference_price DOUBLE PRECISION,
    source TEXT NOT NULL DEFAULT 'signal_dashboard_durum',
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT range_signal_events_dedupe UNIQUE (engine_symbol_id, event_kind, bar_open_time)
);

CREATE INDEX idx_range_signal_events_symbol_bar ON range_signal_events (engine_symbol_id, bar_open_time DESC);
