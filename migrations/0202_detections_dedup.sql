-- 0202_detections_dedup.sql
-- Faz 13.UI — prevent duplicate detections from cross-level and
-- cross-sweep re-insertion. One detection is uniquely identified by
-- (exchange, symbol, timeframe, family, subkind, pivot_level, first-
-- anchor time, last-anchor time). Same pattern re-detected on a later
-- sweep → UPDATE confidence/detected_at/anchors instead of a new row.

BEGIN;

-- ─── 1. Config seed — Elliott level list (GUI-editable) ────────────
INSERT INTO system_config (module, config_key, value, description)
VALUES (
    'detection',
    'elliott.pivot_levels',
    '"L0,L1,L2,L3"'::jsonb,
    'Elliott detector çalışacağı pivot seviyeleri. Virgülle ayrılmış: "L0,L1" ya da "L0,L1,L2,L3". Her seviye bağımsız runner oluşturur.'
)
ON CONFLICT (module, config_key) DO NOTHING;

-- ─── 2. Clean existing duplicates ──────────────────────────────────
-- Elliott + pivot_reversal families get a full wipe (write-amplified
-- pre-Faz13.UI). Other families keep most-recent per identity tuple.
DELETE FROM qtss_v2_detections WHERE family IN ('elliott', 'pivot_reversal');

WITH ranked AS (
    SELECT id,
           row_number() OVER (
               PARTITION BY exchange, symbol, timeframe, family, subkind,
                            COALESCE(pivot_level, ''),
                            (anchors->0->>'time'),
                            (anchors->-1->>'time')
               ORDER BY detected_at DESC, created_at DESC, id DESC
           ) AS rn
      FROM qtss_v2_detections
)
DELETE FROM qtss_v2_detections
 WHERE id IN (SELECT id FROM ranked WHERE rn > 1);

-- ─── 3. Unique index on detection identity ─────────────────────────
-- JSONB expression index — ensures INSERT ... ON CONFLICT works on the
-- (anchors[0].time, anchors[-1].time) tuple. pivot_level may be NULL
-- (legacy rows / pre-Faz12 families); COALESCE collapses that to ''.
CREATE UNIQUE INDEX IF NOT EXISTS ux_v2_detections_identity
ON qtss_v2_detections (
    exchange, symbol, timeframe, family, subkind,
    (COALESCE(pivot_level, '')),
    ((anchors->0->>'time')),
    ((anchors->-1->>'time'))
);

COMMIT;
