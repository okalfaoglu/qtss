-- 0092_wyckoff_pivot_window.sql
--
-- Pivot-window cap for Wyckoff detector. Root cause of the bogus
-- 3621 → 126208 BTC 1d range: detector consumed *every* L1 pivot since
-- inception, so `TradingRange::from_pivots` resolved to ATL↔ATH.
-- Canonical Wyckoff ranges span 15–40 pivots per Villahermosa ch. 4.

SELECT _qtss_register_key(
    'wyckoff.pivot_window', 'wyckoff','range','int',
    '40'::jsonb,'count',
    'Max pivots fed to Wyckoff detector (trailing window). Without a cap, the detector sees all history since asset inception and resolves ranges to ATL↔ATH.',
    'number', true, 'normal', ARRAY['wyckoff','range']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detector','wyckoff.pivot_window','40'::jsonb,
     'Trailing pivot window size fed to Wyckoff detector.')
ON CONFLICT (module, config_key) DO UPDATE SET value = EXCLUDED.value;

-- Fail any remaining wide-range rows so worker rebuilds clean.
UPDATE wyckoff_structures
   SET is_active = false,
       failed_at = now(),
       failure_reason = 'pre-P8: pivot window uncapped, range spanned ATL↔ATH'
 WHERE is_active
   AND range_top > 0
   AND (range_top - range_bottom) / NULLIF((range_top + range_bottom)/2, 0) > 0.6;
