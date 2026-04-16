-- 0106_wyckoff_vp_tp.sql
--
-- P7.4 — Volume-Profile-based TP1 override for Wyckoff trade planner.
-- Villahermosa *Wyckoff 2.0* §7.4.3: "targets cluster at mean-volume
-- reference points — trade to the next node, not to an R-multiple."
-- When enabled, the planner replaces TP1's price with the nearest
-- HVN / naked VPOC / prior swing that lies beyond entry in direction.
-- Subsequent rungs (TP2+, runner) stay as adaptive ATR targets.

SELECT _qtss_register_key(
    'wyckoff.plan.use_vprofile_tp','setup','detection','bool',
    'true'::jsonb, 'flag',
    'Override TP1 with the nearest HVN / naked VPOC / prior-swing level when a volume profile is available.',
    'boolean', true, 'normal', ARRAY['wyckoff','plan','tp','vprofile']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','wyckoff.plan.use_vprofile_tp','true'::jsonb,'Use Volume-Profile HVN/naked-VPOC targets for TP1 when available.')
ON CONFLICT (module, config_key) DO NOTHING;
