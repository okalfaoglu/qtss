-- Wyckoff detector config seed — Faz 14.
--
-- qtss-wyckoff crate: 12 event detectors (PS / SC / AR / ST / Spring /
-- Test / SOS / LPS / BU / BC / UTAD / SOW) + Phase A-E state machine.
-- Engine writer `wyckoff` (12th dispatch member) reads bars, runs
-- `detect_events()`, feeds `WyckoffPhaseTracker`, upserts into
-- `detections` with pattern_family = 'wyckoff' and subkind
-- '<event>_<variant>' (e.g. `sc_bear`, `spring_bull`).

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('wyckoff', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master on/off for the Wyckoff engine writer.'),

    -- Detector thresholds. All read as a single number via JSONB key
    -- `value`; missing rows fall back to `WyckoffConfig::default()` in
    -- the qtss-wyckoff crate.
    ('wyckoff', 'thresholds.climax_volume_mult',
     '{"value": 3.0}'::jsonb,
     'Volume multiplier over the trailing SMA at which a bar qualifies as a Selling/Buying Climax (≥ N× average).'),
    ('wyckoff', 'thresholds.climax_range_atr_mult',
     '{"value": 2.0}'::jsonb,
     'Bar-range / ATR multiplier for SC/BC climax recognition (wide bar relative to recent volatility).'),
    ('wyckoff', 'thresholds.spring_wick_max_pct',
     '{"value": 0.15}'::jsonb,
     'Maximum close-to-low (bull spring) or high-to-close (bear UTAD) wick reclaim percentage for a valid shakeout.'),
    ('wyckoff', 'thresholds.sos_amplifier',
     '{"value": 1.5}'::jsonb,
     'Impulse-range amplifier — SOS / SOW bars must exceed ATR × this factor with volume expansion to qualify.')
ON CONFLICT (module, config_key) DO NOTHING;
