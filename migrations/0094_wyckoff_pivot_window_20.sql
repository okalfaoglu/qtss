-- 0094_wyckoff_pivot_window_20.sql
--
-- Tighten default pivot_window 40 → 20. Production dump showed every
-- Phase-A detection (9 in 10 min) being rejected by the structure-birth
-- h/mid cap because 40 L1 pivots on BTC 1d still spans ~1-2 years.
-- Canonical Wyckoff ranges live in 15-25 pivots (Villahermosa ch. 4).

UPDATE system_config
   SET value = '20'::jsonb,
       description = 'Trailing pivot window fed to Wyckoff detector (20 = canonical range size).'
 WHERE module = 'detector'
   AND config_key = 'wyckoff.pivot_window';

UPDATE config_schema
   SET default_value = '20'::jsonb
 WHERE key = 'wyckoff.pivot_window';
