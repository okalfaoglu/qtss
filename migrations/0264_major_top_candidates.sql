-- FAZ 25.3.E — Major Top composite scorer (symmetric to Major Dip).
--
-- User: "analiz sayfasına dip/tepe puanlaması". Same 8-component
-- framework as major_dip_candidates but inverted polarity: looking
-- for END-of-bullish-move signals (impulse exhaustion + buying
-- climax + bearish divergences + extreme greed + positive funding).
--
-- The two tables share schema; worker writes BOTH each tick (one row
-- per polarity per (symbol, tf, candidate_bar)).

CREATE TABLE IF NOT EXISTS major_top_candidates (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange        TEXT NOT NULL,
    segment         TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    candidate_bar   BIGINT NOT NULL,
    candidate_time  TIMESTAMPTZ NOT NULL,
    candidate_price NUMERIC(38, 18) NOT NULL,
    score           DOUBLE PRECISION NOT NULL,
    components      JSONB NOT NULL,
    verdict         TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS major_top_candidates_uniq_bar
    ON major_top_candidates (exchange, segment, symbol, timeframe, candidate_bar);

CREATE INDEX IF NOT EXISTS major_top_candidates_recent_idx
    ON major_top_candidates (symbol, timeframe, candidate_time DESC);

CREATE INDEX IF NOT EXISTS major_top_candidates_score_idx
    ON major_top_candidates (score DESC, candidate_time DESC);
