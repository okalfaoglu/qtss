-- 0091_wyckoff_tf_range_caps.sql
--
-- Per-TF `max_range_height_pct` caps. Production query showed every
-- active wyckoff_structure row had a range height/midpoint ratio well
-- above the single global 0.15 cap (1d BTC 189%, 1w 189%, 1M ETH 175%)
-- because structure-birth path bypassed `eval_trading_range`'s guard
-- and used raw detection.anchors. Fix lives in v2_detection_orchestrator
-- (commit pairs with this migration); here we seed the caps it reads.

SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.1m',  'wyckoff','range','float',
    '0.05'::jsonb,'fraction','Max range height / midpoint for 1m Wyckoff range.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.3m',  'wyckoff','range','float',
    '0.05'::jsonb,'fraction','Max range height / midpoint for 3m.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.5m',  'wyckoff','range','float',
    '0.05'::jsonb,'fraction','Max range height / midpoint for 5m.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.15m', 'wyckoff','range','float',
    '0.07'::jsonb,'fraction','Max range height / midpoint for 15m.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.30m', 'wyckoff','range','float',
    '0.07'::jsonb,'fraction','Max range height / midpoint for 30m.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.1h',  'wyckoff','range','float',
    '0.10'::jsonb,'fraction','Max range height / midpoint for 1h.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.2h',  'wyckoff','range','float',
    '0.10'::jsonb,'fraction','Max range height / midpoint for 2h.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.4h',  'wyckoff','range','float',
    '0.15'::jsonb,'fraction','Max range height / midpoint for 4h.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.6h',  'wyckoff','range','float',
    '0.15'::jsonb,'fraction','Max range height / midpoint for 6h.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.8h',  'wyckoff','range','float',
    '0.15'::jsonb,'fraction','Max range height / midpoint for 8h.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.12h', 'wyckoff','range','float',
    '0.25'::jsonb,'fraction','Max range height / midpoint for 12h.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.1d',  'wyckoff','range','float',
    '0.25'::jsonb,'fraction','Max range height / midpoint for 1d.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.3d',  'wyckoff','range','float',
    '0.35'::jsonb,'fraction','Max range height / midpoint for 3d.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.1w',  'wyckoff','range','float',
    '0.35'::jsonb,'fraction','Max range height / midpoint for 1w.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);
SELECT _qtss_register_key(
    'wyckoff.max_range_height_pct.1M',  'wyckoff','range','float',
    '0.50'::jsonb,'fraction','Max range height / midpoint for 1M.',
    'number', true, 'normal', ARRAY['wyckoff','tf_cap']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detector','wyckoff.max_range_height_pct.1m',  '0.05'::jsonb,'Max range h/mid for 1m.'),
    ('detector','wyckoff.max_range_height_pct.3m',  '0.05'::jsonb,'Max range h/mid for 3m.'),
    ('detector','wyckoff.max_range_height_pct.5m',  '0.05'::jsonb,'Max range h/mid for 5m.'),
    ('detector','wyckoff.max_range_height_pct.15m', '0.07'::jsonb,'Max range h/mid for 15m.'),
    ('detector','wyckoff.max_range_height_pct.30m', '0.07'::jsonb,'Max range h/mid for 30m.'),
    ('detector','wyckoff.max_range_height_pct.1h',  '0.10'::jsonb,'Max range h/mid for 1h.'),
    ('detector','wyckoff.max_range_height_pct.2h',  '0.10'::jsonb,'Max range h/mid for 2h.'),
    ('detector','wyckoff.max_range_height_pct.4h',  '0.15'::jsonb,'Max range h/mid for 4h.'),
    ('detector','wyckoff.max_range_height_pct.6h',  '0.15'::jsonb,'Max range h/mid for 6h.'),
    ('detector','wyckoff.max_range_height_pct.8h',  '0.15'::jsonb,'Max range h/mid for 8h.'),
    ('detector','wyckoff.max_range_height_pct.12h', '0.25'::jsonb,'Max range h/mid for 12h.'),
    ('detector','wyckoff.max_range_height_pct.1d',  '0.25'::jsonb,'Max range h/mid for 1d.'),
    ('detector','wyckoff.max_range_height_pct.3d',  '0.35'::jsonb,'Max range h/mid for 3d.'),
    ('detector','wyckoff.max_range_height_pct.1w',  '0.35'::jsonb,'Max range h/mid for 1w.'),
    ('detector','wyckoff.max_range_height_pct.1M',  '0.50'::jsonb,'Max range h/mid for 1M.')
ON CONFLICT (module, config_key) DO UPDATE SET value = EXCLUDED.value;

-- Clean the bogus multi-cycle structures currently in the table so the
-- worker rebuilds from scratch with the new cap applied.
DELETE FROM wyckoff_structures
 WHERE is_active
   AND range_top > 0
   AND (range_top - range_bottom) / NULLIF((range_top + range_bottom)/2, 0) > 0.6;
