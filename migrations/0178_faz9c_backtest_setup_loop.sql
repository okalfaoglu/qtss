-- 0178_faz9c_backtest_setup_loop.sql
--
-- Faz 9C — backtest dispatcher config (v2_backtest_setup_loop).
--
-- Why: 453k+ backtest detections in qtss_v2_detections but ~0 backtest
-- setups. The live v2_setup_loop reads fetch_latest_v2_confluence which
-- only carries live rows, so historical detections never pass the gate.
-- The new loop bypasses live confluence + AI + commission gates (design
-- rationale documented in docs/notes/backtest_dispatcher_bug.md) and
-- arms setups directly from detection raw_meta.structural_targets +
-- point-in-time ATR.
--
-- All gates default OFF here so merging this migration alone does not
-- change behaviour. Operator flips `backtest.setup_loop.enabled` in
-- Config GUI to start replaying history into setups.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('backtest', 'setup_loop.enabled', 'false'::jsonb,
   'Backtest setup dispatcher master switch. When true, the loop polls unset backtest detections and arms mode=backtest setups.'),
  ('backtest', 'setup_loop.tick_interval_s', '60'::jsonb,
   'Backtest setup loop polling interval (seconds).'),
  ('backtest', 'setup_loop.batch_size', '200'::jsonb,
   'Max unset backtest detections processed per tick.'),
  ('backtest', 'setup_loop.profile', '"T"'::jsonb,
   'Single profile used for backtest setups (T|Q|D). Keeping one profile avoids 3× slot pressure on the allocator; multi-profile backtest runs get their own faz later.'),
  ('backtest', 'setup_loop.min_confidence', '0.55'::jsonb,
   'Floor on detection.confidence before arming a backtest setup. Mirrors live confluence threshold intent.'),
  ('backtest', 'setup_loop.min_structural_score', '0.60'::jsonb,
   'Floor on detection.structural_score before arming. Backtest bypasses live confluence gate so structural_score is the only shape-quality filter.'),
  ('backtest', 'setup_loop.atr_period', '14'::jsonb,
   'ATR window (bars) for point-in-time stop sizing when structural invalidation is unusable.'),
  ('backtest', 'setup_loop.atr_lookback_bars', '30'::jsonb,
   'How many bars before detection.detected_at to fetch for ATR computation.'),
  ('backtest', 'setup_loop.entry_sl_atr_mult', '1.0'::jsonb,
   'ATR-fallback SL distance multiplier when detection invalidation_price is missing or on the wrong side of entry.'),
  ('backtest', 'setup_loop.target_ref_r', '2.0'::jsonb,
   'ATR-fallback target in R multiples (distance from entry to initial SL).'),
  ('backtest', 'setup_loop.risk_pct', '0.5'::jsonb,
   'Per-trade risk percent stamped on the backtest setup (used by portfolio sizing downstream).'),
  ('backtest', 'setup_loop.skip_if_live_setup_open', 'true'::jsonb,
   'Safety: skip arming a backtest setup when a live/dry setup for the same (ex,sym,tf,profile) is already open. Prevents historical replay from colliding with an in-flight live trade during concurrent dev runs.')
ON CONFLICT (module, config_key) DO NOTHING;
