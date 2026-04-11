-- 0049: Wyckoff structures tracking table (Faz 10).

CREATE TABLE IF NOT EXISTS wyckoff_structures (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol          TEXT NOT NULL,
    interval        TEXT NOT NULL,
    exchange        TEXT NOT NULL DEFAULT 'binance',
    segment         TEXT NOT NULL DEFAULT 'futures',
    schematic       TEXT NOT NULL CHECK (schematic IN (
        'accumulation', 'distribution', 'reaccumulation', 'redistribution'
    )),
    current_phase   TEXT NOT NULL CHECK (current_phase IN ('A','B','C','D','E')),
    range_top       DOUBLE PRECISION,
    range_bottom    DOUBLE PRECISION,
    creek_level     DOUBLE PRECISION,
    ice_level       DOUBLE PRECISION,
    slope_deg       DOUBLE PRECISION DEFAULT 0,
    confidence      DOUBLE PRECISION DEFAULT 0,
    events_json     JSONB NOT NULL DEFAULT '[]',
    volume_profile  JSONB,
    is_active       BOOLEAN DEFAULT true,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at    TIMESTAMPTZ,
    failed_at       TIMESTAMPTZ,
    failure_reason  TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_wyckoff_structures_active
    ON wyckoff_structures (symbol, interval) WHERE is_active = true;
CREATE INDEX IF NOT EXISTS idx_wyckoff_structures_symbol
    ON wyckoff_structures (symbol, started_at DESC);
