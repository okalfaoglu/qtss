-- 0176_faz10_scallop_tighten.sql
--
-- Faz 10 Aşama 4.1 — Scallop detector threshold tightening.
--
-- Why: BTCUSDT 1h showed scallop_bullish on a 3-pivot local swing
-- that was structurally a minor retrace inside a larger distribution
-- range. Original thresholds (R² 0.55, rim progress 2%) were loose
-- enough that noisy pivot triples passed the curvature + asymmetry
-- checks. Failure mode: detection fires, operator sees target
-- projection, price immediately retraces because no real J-shape
-- broke out.
--
-- Changes:
--   * scallop_roundness_r2         0.55 → 0.70  (Bulkowski-closer curvature)
--   * scallop_min_rim_progress_pct 0.02 → 0.035 (stricter breakout asymmetry)
--
-- scallop_min_bars left at 20 — span filter was adequate.

UPDATE system_config
   SET value = '0.70'::jsonb,
       description = 'Scallop curve için parabolic R² eşiği. 0.55→0.70 (Faz10 4.1).'
 WHERE module = 'detection'
   AND config_key = 'classical.scallop_roundness_r2';

UPDATE system_config
   SET value = '0.035'::jsonb,
       description = 'Scallop breakout ayaklık: rim_r rim_l''den en az bu fraksiyon kadar ötede. 0.02→0.035 (Faz10 4.1).'
 WHERE module = 'detection'
   AND config_key = 'classical.scallop_min_rim_progress_pct';
