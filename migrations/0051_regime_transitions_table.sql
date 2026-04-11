-- Faz 11: Regime Deep — regime_transitions table
-- Records detected regime transitions for alerting and backtest validation.

CREATE TABLE IF NOT EXISTS regime_transitions (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol                TEXT NOT NULL,
    interval              TEXT NOT NULL,
    from_regime           TEXT NOT NULL,
    to_regime             TEXT NOT NULL,
    transition_speed      DOUBLE PRECISION,
    confidence            DOUBLE PRECISION NOT NULL,
    confirming_indicators JSONB DEFAULT '[]',
    hmm_probability       DOUBLE PRECISION,
    detected_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at           TIMESTAMPTZ,
    was_correct           BOOLEAN
);

CREATE INDEX IF NOT EXISTS idx_regime_transitions_active
    ON regime_transitions (symbol, interval)
    WHERE resolved_at IS NULL;
