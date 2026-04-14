-- 0065_wyckoff_status_tracking.sql — Faz 10 / status tracker.
--
-- Adds a cursor column so the Wyckoff status loop can resume from the
-- last bar it evaluated instead of re-scanning from setup creation.
-- Also tracks mutable TP-ladder progress by flipping `hit: true` in the
-- existing `tp_ladder` JSONB (no new column needed for that — the hit
-- flag was always part of the ladder schema).
--
-- Why a cursor column rather than timestamp-of-last-row?
--   * Multiple TP hits on one candle must all register as events.
--   * Bars can be backfilled / revised; the cursor lets the tracker
--     advance monotonically per setup regardless of bar revisions.
--
-- Stored as `last_tracked_bar_ts TIMESTAMPTZ`: the `open_time` of the
-- most recent bar already evaluated. NULL → tracker has not yet run
-- against the setup (will start from `created_at`).

ALTER TABLE qtss_v2_setups
  ADD COLUMN IF NOT EXISTS last_tracked_bar_ts TIMESTAMPTZ;

-- Partial index: only open setups need cursor lookups. Closed rows
-- keep the column but the tracker ignores them.
CREATE INDEX IF NOT EXISTS idx_v2_setups_tracker_cursor
  ON qtss_v2_setups (last_tracked_bar_ts)
  WHERE state IN ('armed', 'active');

-- Config knobs for the status loop (#2: no hardcoded constants).
INSERT INTO system_config (module, config_key, value, description) VALUES
  ('setup', 'wyckoff.tracker.enabled', '"true"',
   'Master switch for the Wyckoff status tracker loop.'),
  ('setup', 'wyckoff.tracker.interval_seconds', '"15"',
   'Tick interval for the Wyckoff status tracker (seconds).'),
  ('setup', 'wyckoff.tracker.lookback_bars', '"200"',
   'How many recent bars the tracker fetches when no cursor exists.'),
  ('setup', 'wyckoff.tracker.entry_touch_bps', '"5"',
   'Slack (basis points) around entry_price counted as a touch. 5 = 0.05%.'),
  -- Phase-C manipulation (Spring/UTAD) pre-conditions. Without these
  -- gates every trend pullback low registered as a "Spring".
  ('detector', 'wyckoff.manipulation_min_edge_tests', '"2"',
   'Min prior support/resistance tests (pivots within edge tolerance) required before a Spring/UTAD can fire.'),
  ('detector', 'wyckoff.manipulation_min_range_age_bars', '"10"',
   'Min bars between the first edge test and the Spring/UTAD candidate. Enforces that the range is an established Wyckoff range, not a fleeting pullback.')
ON CONFLICT (module, config_key) DO NOTHING;
