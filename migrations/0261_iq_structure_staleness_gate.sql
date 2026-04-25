-- FAZ 25.x — staleness gate for IQ structure tracker.
--
-- User: "bir itki veya düzeltme tamamlanmışsa o formasyon için işlem
-- açılır mı? açılmaz ise açılmaması için nasıl bir önlem almamız
-- gerekir." Live regression: ETHUSDT 1w spawned a long IQ-D setup at
-- $4098 because the elliott_early detector flagged a "nascent W3"
-- pattern whose W3 anchor was from March 2024 — almost two years old.
-- Spot was $2300 by then, so the setup armed at -45% from inception.
--
-- Fix: iq_structure_tracker_loop now compares the last structural
-- anchor's timestamp against `nascent_max_anchor_age_bars × tf bar
-- duration`. If the anchor is older, the structure is flagged
-- `completed` so the IQ-D candidate loop (filters on
-- `state IN ('candidate','tracking')`) skips it.
--
-- Default 8 bars → ~2 months on 1w, ~8 days on 1d, ~32 hours on 1h.
-- Operators tune live via system_config / GUI.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('iq_structure', 'nascent_max_anchor_age_bars',
     '{"value": 8}'::jsonb,
     'Maximum age (in TF bars) of the most recent structural anchor before iq_structure_tracker flips a candidate/tracking row to ''completed''. Prevents historical Elliott skeletons from being treated as live patterns. ~2 months on 1w, ~8 days on 1d, ~32 hours on 1h.')
ON CONFLICT (module, config_key) DO UPDATE
   SET value = EXCLUDED.value, updated_at = now();

-- One-shot sweep: mark every existing stale candidate/tracking row
-- completed RIGHT NOW so the next IQ-D candidate tick can't open a
-- new setup against them before the worker pulls the new code.
UPDATE iq_structures
   SET state = 'completed',
       current_stage = 'completed',
       completed_at = COALESCE(completed_at, now()),
       invalidation_reason = COALESCE(invalidation_reason,
                                      'anchor too stale (migration sweep)'),
       raw_meta = raw_meta || jsonb_build_object('stale_sweep_migration', true),
       updated_at = now()
 WHERE state IN ('candidate', 'tracking')
   AND (
        -- Last anchor older than 8 weeks for 1w / 8 days for 1d /
        -- 32 hours for 1h / 8 hours for 15m, etc. Computed inline
        -- via a CASE on timeframe.
        (structure_anchors->-1->>'time')::timestamptz <
        now() - (CASE timeframe
                   WHEN '1m'  THEN INTERVAL '8 minutes'
                   WHEN '3m'  THEN INTERVAL '24 minutes'
                   WHEN '5m'  THEN INTERVAL '40 minutes'
                   WHEN '15m' THEN INTERVAL '2 hours'
                   WHEN '30m' THEN INTERVAL '4 hours'
                   WHEN '1h'  THEN INTERVAL '8 hours'
                   WHEN '4h'  THEN INTERVAL '32 hours'
                   WHEN '1d'  THEN INTERVAL '8 days'
                   WHEN '1w'  THEN INTERVAL '8 weeks'
                   ELSE INTERVAL '8 days'
                 END)
       );
