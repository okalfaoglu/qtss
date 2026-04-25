-- FAZ 25 PR-25D — qtss_setups extension for IQ-D / IQ-T.
--
-- The existing T and D profiles stay untouched. Two additions:
--
-- 1. Widen the profile CHECK constraint to accept 'iq_d' and 'iq_t'.
--    Existing rows ('t' / 'd' / 'q') are still valid.
-- 2. Add parent_setup_id so an IQ-T setup can reference the IQ-D
--    setup it was spawned from. NULL is fine — IQ-D setups have no
--    parent, and standalone T/D setups also have no parent.
--
-- The IQ-T candidate creator worker (qtss-worker/src/iq_t_candidate
-- _loop.rs) reads iq_structures rows in state='tracking' whose
-- current_wave is 'B' or 'C' (or W2/W4 of an active impulse), then
-- looks for a micro-impulse on the child timeframe (parent 1d -> 4h,
-- parent 4h -> 15m, etc.) inside the parent's current correction leg.
-- When it finds one, it writes a setup with profile='iq_t' and
-- parent_setup_id = the IQ-D setup that owns the parent structure
-- (when present; standalone IQ-T uses NULL).

ALTER TABLE qtss_setups DROP CONSTRAINT IF EXISTS qtss_setups_profile_check;
ALTER TABLE qtss_setups
    ADD CONSTRAINT qtss_setups_profile_check
        CHECK (profile = ANY (ARRAY['t', 'q', 'd', 'iq_d', 'iq_t']));

ALTER TABLE qtss_setups
    ADD COLUMN IF NOT EXISTS parent_setup_id uuid
        REFERENCES qtss_setups (id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS qtss_setups_parent_idx
    ON qtss_setups (parent_setup_id)
    WHERE parent_setup_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS qtss_setups_iq_profile_state_idx
    ON qtss_setups (profile, state)
    WHERE profile IN ('iq_d', 'iq_t');

-- Config seeds for the IQ-T candidate loop. Tunable from GUI.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('iq_t_candidate', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master switch for the IQ-T candidate creator loop. When false the loop sleeps and skips work.'),
    ('iq_t_candidate', 'tick_secs',
     '{"secs": 60}'::jsonb,
     'Cadence in seconds. Default 60s — balances detection latency against the worker''s overall tick budget.'),
    ('iq_t_candidate', 'min_anchor_score',
     '{"value": 0.40}'::jsonb,
     'Minimum mean fib_proximity score (0..1) on the child motive''s anchors. Below this the micro-impulse shape is too off-canonical to feed an IQ-T entry.'),
    ('iq_t_candidate', 'size_ratio_to_iq_d',
     '{"value": 0.30}'::jsonb,
     'IQ-T sizing as a fraction of its parent IQ-D risk budget. User decision (sizing 1/4-1/3 of IQ-D); default 0.30.'),
    ('iq_t_candidate', 'parent_to_child_tf',
     '{"map": {"1M": "1w", "1w": "1d", "1d": "4h", "4h": "15m", "1h": "5m"}}'::jsonb,
     'Parent IQ-D timeframe -> child timeframe map for IQ-T scanning. ~1:6 to 1:24 ratio (user TF cascade decision).')
ON CONFLICT (module, config_key) DO NOTHING;
