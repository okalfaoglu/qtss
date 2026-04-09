-- Faz 8.0 Setup Engine — config key alignment patch.
--
-- The worker (`v2_setup_loop` + `v2_setup_telegram_loop`) was written
-- with a slightly different key namespace than migration 0032 seeded.
-- This patch adds the keys the worker actually consults so the GUI
-- config editor can surface them. The 0032 keys stay (they document
-- the spec) and will be unified into a single namespace during Faz
-- 8.0.x cleanup before prod deployment.
--
-- Idempotent: every register call uses INSERT … ON CONFLICT DO NOTHING
-- via the `_qtss_register_key` helper.

-- ───────────────────────── master loop knobs ────────────────────────

SELECT _qtss_register_key('setup.enabled', 'setup','loop','bool',
    'false'::jsonb, NULL,
    'Master switch for the Setup Engine worker loop. Reads confluence and arms/updates/closes setups.',
    'toggle', true, 'normal', ARRAY['setup','engine']);

SELECT _qtss_register_key('setup.tick_interval_s', 'setup','loop','int',
    '30'::jsonb, NULL,
    'How often the setup engine loop runs (seconds).',
    'number', true, 'normal', ARRAY['setup','engine']);

SELECT _qtss_register_key('setup.arm.guven_threshold', 'setup','arm','float',
    '0.5'::jsonb, NULL,
    'Minimum confluence guven required to arm a new setup. Below this, candidates are skipped.',
    'number', true, 'normal', ARRAY['setup','engine']);

-- ───────────────────────── per-profile guard config ─────────────────
-- Worker namespace: setup.profile.{t,q,d}.{key}

-- T (short term)
SELECT _qtss_register_key('setup.profile.t.entry_sl_atr_mult', 'setup','profile.t','float',
    '1.0'::jsonb, NULL, 'T profile: initial stop distance in ATR(14) multiples.',
    'number', true, 'normal', ARRAY['setup','profile.t']);
SELECT _qtss_register_key('setup.profile.t.ratchet_interval_secs', 'setup','profile.t','int',
    '900'::jsonb, NULL, 'T profile: minimum seconds between ratchet tightenings (15m default).',
    'number', true, 'normal', ARRAY['setup','profile.t']);
SELECT _qtss_register_key('setup.profile.t.target_ref_r', 'setup','profile.t','float',
    '1.5'::jsonb, NULL, 'T profile: first target distance in R multiples.',
    'number', true, 'normal', ARRAY['setup','profile.t']);
SELECT _qtss_register_key('setup.profile.t.risk_pct', 'setup','profile.t','float',
    '0.25'::jsonb, NULL, 'T profile: per-setup risk as percent of account equity.',
    'number', true, 'normal', ARRAY['setup','profile.t']);
SELECT _qtss_register_key('setup.profile.t.max_concurrent', 'setup','profile.t','int',
    '4'::jsonb, NULL, 'T profile: max concurrent open setups.',
    'number', true, 'normal', ARRAY['setup','profile.t']);
SELECT _qtss_register_key('setup.profile.t.reverse_guven_threshold', 'setup','profile.t','float',
    '0.65'::jsonb, NULL, 'T profile: confluence guven required for a reverse signal to force-close.',
    'number', true, 'normal', ARRAY['setup','profile.t']);

-- Q (short-mid, most market-sensitive)
SELECT _qtss_register_key('setup.profile.q.entry_sl_atr_mult', 'setup','profile.q','float',
    '1.5'::jsonb, NULL, 'Q profile: initial stop distance in ATR(14) multiples.',
    'number', true, 'normal', ARRAY['setup','profile.q']);
SELECT _qtss_register_key('setup.profile.q.ratchet_interval_secs', 'setup','profile.q','int',
    '3600'::jsonb, NULL, 'Q profile: minimum seconds between ratchet tightenings (1h default).',
    'number', true, 'normal', ARRAY['setup','profile.q']);
SELECT _qtss_register_key('setup.profile.q.target_ref_r', 'setup','profile.q','float',
    '2.5'::jsonb, NULL, 'Q profile: first target distance in R multiples.',
    'number', true, 'normal', ARRAY['setup','profile.q']);
SELECT _qtss_register_key('setup.profile.q.risk_pct', 'setup','profile.q','float',
    '0.5'::jsonb, NULL, 'Q profile: per-setup risk as percent of account equity.',
    'number', true, 'normal', ARRAY['setup','profile.q']);
SELECT _qtss_register_key('setup.profile.q.max_concurrent', 'setup','profile.q','int',
    '3'::jsonb, NULL, 'Q profile: max concurrent open setups.',
    'number', true, 'normal', ARRAY['setup','profile.q']);
SELECT _qtss_register_key('setup.profile.q.reverse_guven_threshold', 'setup','profile.q','float',
    '0.55'::jsonb, NULL, 'Q profile: confluence guven required for a reverse signal to force-close (most sensitive).',
    'number', true, 'normal', ARRAY['setup','profile.q']);

-- D (mid term)
SELECT _qtss_register_key('setup.profile.d.entry_sl_atr_mult', 'setup','profile.d','float',
    '2.5'::jsonb, NULL, 'D profile: initial stop distance in ATR(14) multiples.',
    'number', true, 'normal', ARRAY['setup','profile.d']);
SELECT _qtss_register_key('setup.profile.d.ratchet_interval_secs', 'setup','profile.d','int',
    '86400'::jsonb, NULL, 'D profile: minimum seconds between ratchet tightenings (1d default).',
    'number', true, 'normal', ARRAY['setup','profile.d']);
SELECT _qtss_register_key('setup.profile.d.target_ref_r', 'setup','profile.d','float',
    '4.0'::jsonb, NULL, 'D profile: first target distance in R multiples.',
    'number', true, 'normal', ARRAY['setup','profile.d']);
SELECT _qtss_register_key('setup.profile.d.risk_pct', 'setup','profile.d','float',
    '1.0'::jsonb, NULL, 'D profile: per-setup risk as percent of account equity.',
    'number', true, 'normal', ARRAY['setup','profile.d']);
SELECT _qtss_register_key('setup.profile.d.max_concurrent', 'setup','profile.d','int',
    '2'::jsonb, NULL, 'D profile: max concurrent open setups.',
    'number', true, 'normal', ARRAY['setup','profile.d']);
SELECT _qtss_register_key('setup.profile.d.reverse_guven_threshold', 'setup','profile.d','float',
    '0.7'::jsonb, NULL, 'D profile: confluence guven required for a reverse signal to force-close.',
    'number', true, 'normal', ARRAY['setup','profile.d']);

-- ───────────────────────── risk allocator ───────────────────────────

SELECT _qtss_register_key('setup.risk.total_risk_pct', 'setup','risk','float',
    '6.0'::jsonb, NULL,
    'Cap on total open risk across all setups (sum of per-setup risk_pct), in percent of equity.',
    'number', true, 'normal', ARRAY['setup','risk']);

-- (max_per_group / same_direction_only / venue.{crypto,bist}.enabled
--  already exist in migration 0032 with compatible defaults — no
--  duplicate registration.)
