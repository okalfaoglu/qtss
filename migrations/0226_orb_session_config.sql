-- ORB (Opening Range Breakout) detector config + session boundaries.
-- Activates `OrbWriter` in qtss-engine and seeds session hour bands
-- the classifier in `qtss_regime::session` falls back to when nothing
-- is seeded.

-- ── ORB ───────────────────────────────────────────────────────────────
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('orb', 'enabled',           '{"enabled": true}'::jsonb,
     'Master on/off for the Opening Range Breakout engine writer.'),
    ('orb', 'bars_per_tick',     '{"bars": 2000}'::jsonb,
     'Upper bound on recent bars loaded per symbol per tick.'),
    ('orb', 'or_bars',           '{"value": 4}'::jsonb,
     'Number of bars that form the Opening Range after each session open. 4 on 15m = first hour. Crabel-style 30m ORs on 5m = 6.'),
    ('orb', 'confirm_lookback',  '{"value": 12}'::jsonb,
     'Bars after the OR window to watch for a breakout close. 12 on 15m = 3 hours — keeps the pattern relevant to the current session.'),
    ('orb', 'breakout_atr_mult', '{"value": 0.10}'::jsonb,
     'Minimum breakout magnitude beyond OR boundary as an ATR multiple. Filters tick-by-tick noise; Crabel himself used 0 but ATR ≥ 10% separates real moves from rounding error.'),
    ('orb', 'volume_spike_mult', '{"value": 1.2}'::jsonb,
     'Breakout bar volume must be ≥ this × SMA(volume, 20) to set `volume_confirmed`. The signal still fires without it, but the score drops.'),
    ('orb', 'enabled_sessions',  '{"names": ["asia", "london", "new_york"]}'::jsonb,
     'Which session opens to track. Set to a subset (e.g. ["london","new_york"]) when a strategy should skip Asia opens on thin-liquidity days.')
ON CONFLICT (module, config_key) DO NOTHING;

-- ── Session classifier (future tunable — current code uses hardcoded
-- UTC boundaries; this is the config surface for moving them). Values
-- are stored but not yet read by `qtss_regime::session`; PR-11H wires
-- the reader.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('session', 'asia.start_hour',     '{"value": 0}'::jsonb,  'Asia session start hour UTC.'),
    ('session', 'asia.end_hour',       '{"value": 8}'::jsonb,  'Asia session end hour UTC (exclusive).'),
    ('session', 'london.start_hour',   '{"value": 8}'::jsonb,  'London session start hour UTC.'),
    ('session', 'london.end_hour',     '{"value": 16}'::jsonb, 'London session end hour UTC (exclusive).'),
    ('session', 'new_york.start_hour', '{"value": 13}'::jsonb, 'New York session start hour UTC (begins inside London → overlap).'),
    ('session', 'new_york.end_hour',   '{"value": 22}'::jsonb, 'New York session end hour UTC (exclusive).'),
    ('session', 'overlap.start_hour',  '{"value": 13}'::jsonb, 'London–NY overlap start hour UTC (highest crypto liquidity window).'),
    ('session', 'overlap.end_hour',    '{"value": 16}'::jsonb, 'London–NY overlap end hour UTC.')
ON CONFLICT (module, config_key) DO NOTHING;
