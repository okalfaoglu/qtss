-- 0021_qtss_v2_detection_enable.sql
--
-- Faz 7 — Flip the v2 detection orchestrator + validator on by default.
-- Both keys are seeded as `false` in 0019/0020 so a fresh deploy stays
-- silent until an operator opts in. This migration is the opt-in step
-- for the rollout: it only touches the two master switches, leaving
-- per-family toggles, intervals and thresholds at their seeded defaults
-- so they remain GUI-tunable per CLAUDE.md #2.
--
-- Idempotent: ON CONFLICT DO UPDATE so re-running this migration (or
-- running it after an operator toggled the keys via the GUI) is safe.

INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('detection', 'orchestrator.enabled', 'true'::jsonb,
     'Master switch for the v2 detection orchestrator loop in qtss-worker.'),
    ('detection', 'validator.enabled', 'true'::jsonb,
     'Master switch for the v2 detection validator loop in qtss-worker.')
ON CONFLICT (module, config_key) DO UPDATE
    SET value = EXCLUDED.value,
        updated_at = now();
