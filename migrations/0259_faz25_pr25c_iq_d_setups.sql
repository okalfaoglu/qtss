-- FAZ 25 PR-25C — IQ-D candidate creator config seeds.
--
-- The PR-25C worker (qtss-worker/src/iq_d_candidate_loop.rs) reads
-- iq_structures rows in state candidate/tracking with current_wave
-- W1/W2/W3 and writes qtss_setups rows with profile='iq_d'. This
-- file just seeds the operator-tunable knobs.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('iq_d_candidate', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master switch for the IQ-D candidate creator loop.'),
    ('iq_d_candidate', 'tick_secs',
     '{"secs": 90}'::jsonb,
     'Cadence in seconds. Default 90s — same as the structure tracker so a freshly-promoted structure gets a setup row in the next tick.'),
    ('iq_d_candidate', 'min_anchor_score',
     '{"value": 0.40}'::jsonb,
     'Minimum mean fib_proximity (0..1) for the parent structure''s anchors to be eligible. Below this the Elliott shape is too off-canonical to entry on.'),
    ('iq_d_candidate', 'tier_priority',
     '{"order": ["W1","W2","W3"]}'::jsonb,
     'Preferred entry tier order. The loop only writes a row for the highest-priority tier matching the current_wave — W1 entries (best R:R, hardest to confirm) win over W2 / W3 fallbacks.')
ON CONFLICT (module, config_key) DO NOTHING;
