-- Wyckoff Setup Engine integration (Faz 8.0a)
-- Extends qtss_v2_setups for Wyckoff alt_types, dynamic TP ladder, and dry/live/backtest modes.
--
-- 1. `mode` column on setups (dry|live|backtest) — runtime context, not feature flag.
-- 2. `tp_ladder` JSONB — adaptive TP list [{r:1.0, price:65000, hit:false}, ...].
--    Replaces single `target_ref` for multi-TP partial-close strategies.
-- 3. Extend `alt_type` CHECK to include Wyckoff setup types.
-- 4. Seed Wyckoff-specific Setup Engine config (CLAUDE.md #2 — no hardcoded constants).

BEGIN;

-- ── 1. mode column ──────────────────────────────────────────────
ALTER TABLE qtss_v2_setups
  ADD COLUMN IF NOT EXISTS mode TEXT NOT NULL DEFAULT 'dry'
    CHECK (mode IN ('dry', 'live', 'backtest'));

CREATE INDEX IF NOT EXISTS idx_v2_setups_mode_state
  ON qtss_v2_setups (mode, state)
  WHERE state IN ('armed', 'active');

-- Same for setup_events outbox so consumers can filter
ALTER TABLE qtss_v2_setup_events
  ADD COLUMN IF NOT EXISTS mode TEXT NOT NULL DEFAULT 'dry'
    CHECK (mode IN ('dry', 'live', 'backtest'));

-- ── 2. Dynamic TP ladder ────────────────────────────────────────
-- JSONB schema: [{ "r": 1.0, "price": 65000.0, "hit": false, "hit_at": null, "qty_pct": 33.0 }]
ALTER TABLE qtss_v2_setups
  ADD COLUMN IF NOT EXISTS tp_ladder JSONB NOT NULL DEFAULT '[]'::jsonb;

-- Backtest-specific result columns (filled when state→closed in backtest mode)
ALTER TABLE qtss_v2_setups
  ADD COLUMN IF NOT EXISTS bars_to_first_tp INT,
  ADD COLUMN IF NOT EXISTS bars_to_close INT,
  ADD COLUMN IF NOT EXISTS max_favorable_r REAL,
  ADD COLUMN IF NOT EXISTS max_adverse_r REAL;

-- ── 3. Extend alt_type to allow Wyckoff setup names ─────────────
ALTER TABLE qtss_v2_setups
  DROP CONSTRAINT IF EXISTS qtss_v2_setups_alt_type_check;

ALTER TABLE qtss_v2_setups
  ADD CONSTRAINT qtss_v2_setups_alt_type_check
  CHECK (alt_type IN (
    -- Existing D/T/Q generic types
    'reaction_low', 'trend_low', 'reversal_high', 'selling_high',
    -- Wyckoff accumulation setups (long)
    'wyckoff_spring', 'wyckoff_lps', 'wyckoff_buec',
    -- Wyckoff distribution setups (short)
    'wyckoff_ut', 'wyckoff_utad', 'wyckoff_lpsy', 'wyckoff_ice_retest'
  ));

-- ── 4. Seed Wyckoff Setup Engine config ─────────────────────────
INSERT INTO system_config (module, config_key, value, description) VALUES
  -- Master switch
  ('setup', 'wyckoff.enabled', '"true"', 'Master switch for Wyckoff alt_type in Setup Engine'),
  ('setup', 'wyckoff.scan.interval_seconds', '"30"', 'How often the wyckoff signal loop scans symbols'),
  ('setup', 'wyckoff.scan.timeframes', '["1h","4h"]', 'Timeframes scanned for Wyckoff setups (D=4h decision/1h trigger, Q=1h)'),
  ('setup', 'wyckoff.scan.modes', '["dry"]', 'Active modes for Wyckoff loop (dry|live|backtest). Add live to enable real-money path.'),

  -- Profile mapping: which Wyckoff setup → which D/T/Q profile
  ('setup', 'wyckoff.profile_map.spring', '"d"', 'Spring (Phase C accumulation) → D profile (orta vade)'),
  ('setup', 'wyckoff.profile_map.lps', '"q"', 'LPS (Phase D pullback) → Q profile (kısa-orta)'),
  ('setup', 'wyckoff.profile_map.buec', '"q"', 'BUEC (creek breakout retest) → Q profile'),
  ('setup', 'wyckoff.profile_map.ut', '"d"', 'UT/UTAD (Phase C distribution) → D profile'),
  ('setup', 'wyckoff.profile_map.lpsy', '"q"', 'LPSY (Phase D distribution pullback) → Q profile'),
  ('setup', 'wyckoff.profile_map.ice_retest', '"q"', 'Ice break + retest → Q profile'),

  -- Setup quality gates
  ('setup', 'wyckoff.setup.min_phase', '"C"', 'Only emit signals when range phase >= C'),
  ('setup', 'wyckoff.setup.min_range_bars', '"20"', 'Trading range must have at least N bars'),
  ('setup', 'wyckoff.setup.min_climax_volume_ratio', '"1.8"', 'SC/BC volume must be >= N x 20-bar avg'),
  ('setup', 'wyckoff.setup.allowed_types', '["spring","lps","buec","ut","utad","lpsy","ice_retest"]', 'Whitelist of Wyckoff setup types to emit'),
  ('setup', 'wyckoff.setup.min_score', '"60"', 'Minimum composite score (0-100) to emit setup'),

  -- Adaptive TP ladder (volatility-bucketed)
  -- Each bucket: { atr_pct_max, tp_count, multipliers, qty_split }
  ('setup', 'wyckoff.tp.adaptive.buckets',
    '[
      {"atr_pct_max": 1.0,  "tp_count": 4, "r_multipliers": [0.8, 1.5, 2.5, 4.0], "qty_split_pct": [25, 25, 25, 25], "label": "low_vol"},
      {"atr_pct_max": 3.0,  "tp_count": 3, "r_multipliers": [1.0, 1.8, 3.0],      "qty_split_pct": [33, 33, 34],     "label": "mid_vol"},
      {"atr_pct_max": 99.0, "tp_count": 2, "r_multipliers": [1.2, 2.5],           "qty_split_pct": [50, 50],         "label": "high_vol"}
    ]',
    'Adaptive TP ladder buckets keyed by symbol ATR% — system picks tp_count+multipliers automatically'),

  -- TP/range cap (Wyckoff Cause/Effect: TP cannot exceed range_height * factor)
  ('setup', 'wyckoff.tp.range_cap_factor', '"1.5"', 'TP price cannot exceed range_height * factor (P&F count proxy)'),
  ('setup', 'wyckoff.tp.score_boost_threshold', '"75"', 'When score >= N, append +1 TP for runner'),
  ('setup', 'wyckoff.tp.score_boost_r', '"5.0"', 'R multiplier of the boosted runner TP'),

  -- Net-RR gate (after commission)
  ('setup', 'wyckoff.tp.min_net_rr', '"1.0"', 'Reject setup if weighted expected net R (after 2x commission, qty-split-weighted) < N'),

  -- Signal lifecycle
  ('setup', 'wyckoff.signal.ttl_bars', '"24"', 'Pending signal expires if entry not triggered within N bars'),
  ('setup', 'wyckoff.signal.invalidate_on_phase_e', '"true"', 'Cancel pending/active signal when range moves to Phase E'),

  -- Risk per profile (overrides generic setup.guard.* if present, otherwise inherits)
  ('setup', 'wyckoff.risk.d.entry_sl_atr_mult', '"2.5"', 'D-profile Wyckoff: SL = entry +/- N * ATR'),
  ('setup', 'wyckoff.risk.q.entry_sl_atr_mult', '"1.5"', 'Q-profile Wyckoff: SL = entry +/- N * ATR'),

  -- Backtest knobs
  ('setup', 'wyckoff.backtest.from_iso', '"2023-01-01T00:00:00Z"', 'Default backtest start (overridable per run)'),
  ('setup', 'wyckoff.backtest.to_iso', '"2025-12-31T23:59:59Z"', 'Default backtest end'),
  ('setup', 'wyckoff.backtest.slippage_bps', '"5"', 'Backtest entry/exit slippage in basis points'),

  -- ATR period (used by adaptive bucket selection)
  ('setup', 'wyckoff.atr.period', '"14"', 'ATR period for volatility bucketing and SL distance')
ON CONFLICT (module, config_key) DO NOTHING;

COMMIT;
