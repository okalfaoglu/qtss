-- Faz 11: Regime Deep — regime_snapshots table
-- Stores per-symbol, per-interval regime classification snapshots.

CREATE TABLE IF NOT EXISTS regime_snapshots (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol        TEXT NOT NULL,
    interval      TEXT NOT NULL,
    regime        TEXT NOT NULL,
    trend_strength TEXT,
    confidence    DOUBLE PRECISION NOT NULL,
    adx           DOUBLE PRECISION,
    plus_di       DOUBLE PRECISION,
    minus_di      DOUBLE PRECISION,
    bb_width      DOUBLE PRECISION,
    atr_pct       DOUBLE PRECISION,
    choppiness    DOUBLE PRECISION,
    hmm_state     TEXT,
    hmm_confidence DOUBLE PRECISION,
    computed_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_regime_snapshots_lookup
    ON regime_snapshots (symbol, interval, computed_at DESC);
