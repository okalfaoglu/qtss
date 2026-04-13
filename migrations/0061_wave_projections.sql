-- Wave Projections: future formation predictions with alternatives & validation tracking.
-- Each row is ONE alternative scenario originating from a completed/active wave.

CREATE TABLE IF NOT EXISTS wave_projections (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Source: which wave triggered this projection
    source_wave_id    UUID NOT NULL REFERENCES wave_chain(id) ON DELETE CASCADE,
    alt_group         UUID NOT NULL,            -- groups alternatives from same source point

    -- Identity
    exchange          TEXT NOT NULL,
    symbol            TEXT NOT NULL,
    timeframe         TEXT NOT NULL,
    degree            TEXT NOT NULL,             -- 'Cycle', 'Primary', etc.

    -- What we predict
    projected_kind    TEXT NOT NULL,             -- 'zigzag_abc', 'flat_expanded', 'triangle_contracting', etc.
    projected_label   TEXT NOT NULL,             -- human: "Wave 4 — Flat (Expanded)"
    direction         TEXT NOT NULL,             -- 'bullish' / 'bearish'

    -- Fibonacci basis
    fib_basis         TEXT,                      -- '0.382 retrace', '1.618 extension', etc.

    -- All projected legs in one JSON array
    -- [{label, price_start, price_end, time_start_est, time_end_est, fib_level}]
    projected_legs    JSONB NOT NULL DEFAULT '[]'::jsonb,

    -- Probability & ranking
    probability       REAL NOT NULL DEFAULT 0.5, -- 0.0-1.0
    rank              INT NOT NULL DEFAULT 1,     -- 1 = most likely

    -- Validation state
    state             TEXT NOT NULL DEFAULT 'active',
        -- 'active'      = still possible
        -- 'leading'     = highest probability among active alternatives
        -- 'confirmed'   = new detection matched this projection
        -- 'eliminated'  = invalidated by price/time
    elimination_reason TEXT,                     -- 'price_breach_w1', 'time_exceeded', 'superseded', etc.

    -- Validation tracking
    bars_validated    INT NOT NULL DEFAULT 0,
    last_validated_at TIMESTAMPTZ,
    confirmed_detection_id UUID,                -- if confirmed, link to the actual detection

    -- Time estimates
    time_start_est    TIMESTAMPTZ,              -- estimated start of projected formation
    time_end_est      TIMESTAMPTZ,              -- estimated end

    -- Price targets (summary)
    price_target_min  NUMERIC,                  -- lowest projected price
    price_target_max  NUMERIC,                  -- highest projected price
    invalidation_price NUMERIC,                 -- if price crosses this, eliminate

    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Fast lookups
CREATE INDEX IF NOT EXISTS idx_wp_source      ON wave_projections (source_wave_id);
CREATE INDEX IF NOT EXISTS idx_wp_alt_group   ON wave_projections (alt_group);
CREATE INDEX IF NOT EXISTS idx_wp_active      ON wave_projections (exchange, symbol, timeframe, state)
    WHERE state IN ('active', 'leading');
CREATE INDEX IF NOT EXISTS idx_wp_series      ON wave_projections (exchange, symbol, timeframe, created_at DESC);
