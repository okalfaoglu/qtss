-- 0089_wyckoff_progression_fix.sql
--
-- Faz 10 / P28a–c — Wyckoff structure progression fix.
--
-- Symptoms observed in prod (2026-04-15): 477 Phase A on 15m BTCUSDT,
-- 476 failed with "structure TTL exceeded (29702 bars since last
-- event; limit 400)" or "schematic flipped accumulation →
-- redistribution via UTAD". No structure ever progressed past Phase A.
--
-- Root causes fixed in code:
--   P28a) TTL age was computed as (global bar_index - persisted
--         bar_index), but persisted bar_index was written under a
--         rolling-window tracker → huge drift (29702 bars = ~309d).
--         Switched to time_ms (already on RecordedEvent).
--   P28b) Seed-from-event heuristic mapped Spring→Accumulation and
--         UTAD→Distribution, seeding a structure on a bare Phase C
--         event. auto_reclassify then immediately flipped it. Now
--         seeding refuses non-Phase-A events and uses detection.variant
--         authoritatively.
--   P28c) A→B gate required SC+AR+ST all three; ST is often deduped
--         into SC on fast TFs. Relaxed to "climax + (AR or ST)".
--
-- This migration only seeds the config table — the TTL and gate
-- thresholds now live in system_config so operators can tune them
-- without a redeploy (CLAUDE.md #2).

SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.1m',  'wyckoff','structure','int',  '28800'::jsonb,   'seconds', 'Structure TTL (seconds) on 1m TF — no new event within this window fails the structure.',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.3m',  'wyckoff','structure','int',  '90000'::jsonb,   'seconds', 'Structure TTL (seconds) on 3m TF.',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.5m',  'wyckoff','structure','int',  '150000'::jsonb,  'seconds', 'Structure TTL (seconds) on 5m TF.',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.15m', 'wyckoff','structure','int',  '345600'::jsonb,  'seconds', 'Structure TTL (seconds) on 15m TF (~4 days).',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.30m', 'wyckoff','structure','int',  '691200'::jsonb,  'seconds', 'Structure TTL (seconds) on 30m TF (~8 days).',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.1h',  'wyckoff','structure','int',  '1468800'::jsonb, 'seconds', 'Structure TTL (seconds) on 1h TF (~17 days).',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.2h',  'wyckoff','structure','int',  '2520000'::jsonb, 'seconds', 'Structure TTL (seconds) on 2h TF.',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.4h',  'wyckoff','structure','int',  '4320000'::jsonb, 'seconds', 'Structure TTL (seconds) on 4h TF (~50 days).',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.6h',  'wyckoff','structure','int',  '5400000'::jsonb, 'seconds', 'Structure TTL (seconds) on 6h TF.',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.8h',  'wyckoff','structure','int',  '7200000'::jsonb, 'seconds', 'Structure TTL (seconds) on 8h TF.',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.12h', 'wyckoff','structure','int',  '8640000'::jsonb, 'seconds', 'Structure TTL (seconds) on 12h TF.',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.1d',  'wyckoff','structure','int',  '10368000'::jsonb,'seconds', 'Structure TTL (seconds) on 1d TF (~4 months).',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.3d',  'wyckoff','structure','int',  '20736000'::jsonb,'seconds', 'Structure TTL (seconds) on 3d TF.',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.1w',  'wyckoff','structure','int',  '48384000'::jsonb,'seconds', 'Structure TTL (seconds) on 1w TF.',  'number', true, 'normal', ARRAY['wyckoff','structure']);
SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.1M',  'wyckoff','structure','int',  '93312000'::jsonb,'seconds', 'Structure TTL (seconds) on 1M TF.',  'number', true, 'normal', ARRAY['wyckoff','structure']);

SELECT _qtss_register_key('wyckoff.structure.ttl_seconds.default', 'wyckoff','structure','int', '4320000'::jsonb, 'seconds',
    'Fallback TTL (seconds) for timeframes without a specific override.',
    'number', true, 'normal', ARRAY['wyckoff','structure']);

SELECT _qtss_register_key('wyckoff.phase.a_to_b.require_st', 'wyckoff','phase','bool',
    'false'::jsonb, NULL,
    'When true, Phase A → B requires both AR and ST. When false (default post-P28c), climax + (AR OR ST) is sufficient — ST is often deduped into SC on fast TFs.',
    'toggle', true, 'normal', ARRAY['wyckoff','phase']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('wyckoff','structure.ttl_seconds.1m',   '28800'::jsonb,   'Wyckoff structure TTL on 1m (s).'),
    ('wyckoff','structure.ttl_seconds.3m',   '90000'::jsonb,   'Wyckoff structure TTL on 3m (s).'),
    ('wyckoff','structure.ttl_seconds.5m',   '150000'::jsonb,  'Wyckoff structure TTL on 5m (s).'),
    ('wyckoff','structure.ttl_seconds.15m',  '345600'::jsonb,  'Wyckoff structure TTL on 15m (s).'),
    ('wyckoff','structure.ttl_seconds.30m',  '691200'::jsonb,  'Wyckoff structure TTL on 30m (s).'),
    ('wyckoff','structure.ttl_seconds.1h',   '1468800'::jsonb, 'Wyckoff structure TTL on 1h (s).'),
    ('wyckoff','structure.ttl_seconds.2h',   '2520000'::jsonb, 'Wyckoff structure TTL on 2h (s).'),
    ('wyckoff','structure.ttl_seconds.4h',   '4320000'::jsonb, 'Wyckoff structure TTL on 4h (s).'),
    ('wyckoff','structure.ttl_seconds.6h',   '5400000'::jsonb, 'Wyckoff structure TTL on 6h (s).'),
    ('wyckoff','structure.ttl_seconds.8h',   '7200000'::jsonb, 'Wyckoff structure TTL on 8h (s).'),
    ('wyckoff','structure.ttl_seconds.12h',  '8640000'::jsonb, 'Wyckoff structure TTL on 12h (s).'),
    ('wyckoff','structure.ttl_seconds.1d',   '10368000'::jsonb,'Wyckoff structure TTL on 1d (s).'),
    ('wyckoff','structure.ttl_seconds.3d',   '20736000'::jsonb,'Wyckoff structure TTL on 3d (s).'),
    ('wyckoff','structure.ttl_seconds.1w',   '48384000'::jsonb,'Wyckoff structure TTL on 1w (s).'),
    ('wyckoff','structure.ttl_seconds.1M',   '93312000'::jsonb,'Wyckoff structure TTL on 1M (s).'),
    ('wyckoff','structure.ttl_seconds.default', '4320000'::jsonb, 'Wyckoff structure TTL fallback (s).'),
    ('wyckoff','phase.a_to_b.require_st',    'false'::jsonb,   'If true, A→B requires AR AND ST; default false accepts AR OR ST.')
ON CONFLICT (module, config_key) DO NOTHING;
