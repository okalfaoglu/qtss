-- 0040 — Detection outcomes: track win/loss/scratch for validator self-learning.
--
-- Two additions:
-- 1. qtss_v2_detection_outcomes — one row per resolved detection
-- 2. qtss_v2_setups.detection_id — FK back to the originating detection

-- ─── 1. detection_id on setups ──────────────────────────────────────────

ALTER TABLE qtss_v2_setups
    ADD COLUMN IF NOT EXISTS detection_id UUID REFERENCES qtss_v2_detections(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_v2_setups_detection
    ON qtss_v2_setups (detection_id) WHERE detection_id IS NOT NULL;

-- Backfill from raw_meta where available.
UPDATE qtss_v2_setups
SET    detection_id = (raw_meta ->> 'detection_id')::uuid
WHERE  detection_id IS NULL
  AND  raw_meta ->> 'detection_id' IS NOT NULL;

-- ─── 2. detection_outcomes table ────────────────────────────────────────

CREATE TABLE IF NOT EXISTS qtss_v2_detection_outcomes (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    detection_id    UUID NOT NULL REFERENCES qtss_v2_detections(id) ON DELETE CASCADE,
    setup_id        UUID REFERENCES qtss_v2_setups(id) ON DELETE SET NULL,
    outcome         TEXT NOT NULL CHECK (outcome IN ('win','loss','scratch','expired')),
    close_reason    TEXT,          -- target_hit, stop_hit, reverse_signal, manual, timeout
    pnl_pct         REAL,          -- realized P&L as % of risk
    entry_price     REAL,
    exit_price      REAL,
    duration_secs   BIGINT,        -- detection → close elapsed seconds
    resolved_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_detection_outcome UNIQUE (detection_id)
);

CREATE INDEX IF NOT EXISTS idx_detection_outcomes_family
    ON qtss_v2_detection_outcomes (outcome, resolved_at DESC);

-- Partial index for the validator hit-rate query.
CREATE INDEX IF NOT EXISTS idx_detection_outcomes_for_hitrate
    ON qtss_v2_detection_outcomes (outcome)
    INCLUDE (detection_id);
