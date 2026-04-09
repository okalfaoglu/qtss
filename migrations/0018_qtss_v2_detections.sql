-- 0018_qtss_v2_detections.sql
--
-- Faz 7 Adım 1 — Detection persistence.
--
-- One row per pattern detection emitted by the v2 detector orchestrator
-- (Elliott / Harmonic / Classical / Wyckoff / Range / Custom). The
-- validator (Adım 3) updates the same row with confidence + channel
-- scores rather than writing a separate `validated_detections` table —
-- a detection's lifecycle (forming → confirmed/invalidated) lives in
-- the `state` column so the chart endpoint and Detections panel can
-- query a single source of truth.
--
-- The orchestrator writes here from the qtss-worker (live/dry/backtest
-- aware via `mode`). The validator updates `confidence`,
-- `channel_scores`, `validated_at` once it has run its checks.
--
-- Indexes are tuned for the two read patterns we know we need:
--   1. Chart overlay: latest N detections for (exchange, symbol, tf).
--   2. Detections panel feed: latest N filtered by family/state.

CREATE TABLE IF NOT EXISTS qtss_v2_detections (
    id                  uuid        PRIMARY KEY,
    detected_at         timestamptz NOT NULL,
    exchange            text        NOT NULL,
    symbol              text        NOT NULL,
    timeframe           text        NOT NULL,
    family              text        NOT NULL,   -- elliott|harmonic|classical|wyckoff|range|custom
    subkind             text        NOT NULL,
    state               text        NOT NULL CHECK (state IN ('forming','confirmed','invalidated','completed')),
    structural_score    real        NOT NULL,
    confidence          real,                  -- nullable until validator runs
    invalidation_price  numeric     NOT NULL,
    anchors             jsonb       NOT NULL,  -- Vec<PivotRef>
    regime              jsonb       NOT NULL,  -- RegimeSnapshot at detection time
    channel_scores      jsonb,                 -- Vec<ChannelScore> from validator
    raw_meta            jsonb       NOT NULL DEFAULT '{}'::jsonb,
    validated_at        timestamptz,
    mode                text        NOT NULL CHECK (mode IN ('live','dry','backtest')),
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS qtss_v2_detections_chart_idx
    ON qtss_v2_detections (exchange, symbol, timeframe, detected_at DESC);

CREATE INDEX IF NOT EXISTS qtss_v2_detections_feed_idx
    ON qtss_v2_detections (detected_at DESC, family, state);

CREATE INDEX IF NOT EXISTS qtss_v2_detections_open_idx
    ON qtss_v2_detections (state, detected_at DESC)
    WHERE state IN ('forming','confirmed');

-- updated_at trigger: keep it fresh on every UPDATE so the validator
-- step writes a meaningful timestamp without having to remember.
CREATE OR REPLACE FUNCTION qtss_v2_detections_set_updated_at()
RETURNS trigger AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS qtss_v2_detections_updated_at ON qtss_v2_detections;
CREATE TRIGGER qtss_v2_detections_updated_at
    BEFORE UPDATE ON qtss_v2_detections
    FOR EACH ROW
    EXECUTE FUNCTION qtss_v2_detections_set_updated_at();
