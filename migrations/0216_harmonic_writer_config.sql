-- Harmonic writer config. Same pattern as detections / zigzag knobs.
-- The writer runs through all 13 patterns in qtss_harmonic::PATTERNS
-- (Gartley, Bat, Butterfly, Crab, Deep Crab, Shark, Cypher, Alt Bat,
-- 5-0, AB=CD, Alt AB=CD, Three Drives) on every 5-pivot window per
-- slot, and only persists rows whose structural score clears
-- `min_score`. Slack loosens every RatioRange by a fractional
-- percentage — CLAUDE.md #2 (no hardcoded tolerances).

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('harmonic', 'enabled',
        '{"enabled": true}'::jsonb,
        'Master switch for the harmonic_writer_loop. Active by default — user asked for harmonic patterns to land in detections table.'),
    ('harmonic', 'tick_secs',
        '{"secs": 60}'::jsonb,
        'Polling cadence in seconds, matches the detections + pivot_writer loops.'),
    ('harmonic', 'min_score',
        '{"score": 0.60}'::jsonb,
        'Minimum structural score (Gaussian match quality, 0..1) for a harmonic to persist. 0.60 filters noisy near-misses while keeping solid matches.'),
    ('harmonic', 'slack',
        '{"slack": 0.05}'::jsonb,
        'Fractional loosening of every RatioRange (5 percent default). Matches the default in qtss_harmonic::HarmonicConfig.'),
    ('harmonic', 'pivots_per_slot',
        '{"count": 500}'::jsonb,
        'How many recent pivots per (series, slot) to scan each tick. 500 covers months on 4h / days on 5m at typical swing density.')
ON CONFLICT (module, config_key) DO NOTHING;
