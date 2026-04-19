-- 0179_faz10_scallop_score_weights.sql
--
-- Faz 10 Aşama 4.3 — scallop confidence score weights to system_config.
--
-- Why: shapes.rs was hardcoding 0.35/0.25/0.25/0.15 for
-- curvature/progress/breakout/volume (documented in
-- docs/notes/scallop_detection_quality.md item 3). CLAUDE.md #2 forbids
-- hardcoded scoring constants — calibration against historical backtest
-- hit-rate requires the operator to tune weights via Config GUI without
-- redeploy. Detector renormalises sums at runtime so edits that drift
-- from 1.0 don't break the clamp.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detection', 'classical.scallop_score_w_curvature', '0.35'::jsonb,
   'Scallop confidence: parabolic R² (curvature) weight. Bulkowski J-shape requires a tight curve; this dominates the score. Calibration target: sweep 0.25..0.50 against /v2/backtest/summary?alt_type_like=scallop_%.'),
  ('detection', 'classical.scallop_score_w_progress', '0.25'::jsonb,
   'Scallop confidence: rim progression (asymmetry) weight. Bigger right rim = stronger directional intent.'),
  ('detection', 'classical.scallop_score_w_breakout', '0.25'::jsonb,
   'Scallop confidence: breakout magnitude (close vs RimR ± ATR) weight. Gates false pivots that never follow through.'),
  ('detection', 'classical.scallop_score_w_volume', '0.15'::jsonb,
   'Scallop confidence: breakout bar volume (vs trailing avg) weight. Lowest because crypto volume data varies by venue.')
ON CONFLICT (module, config_key) DO NOTHING;
