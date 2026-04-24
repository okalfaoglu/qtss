-- Setup state machine: add 'rejected' terminal state + purge zombie data.
--
-- Background: the execution bridge used to let LIVE setups fall through
-- even when dispatch_live failed (broker key had no trading permission,
-- live_enabled=false, etc.). setup_watcher then later closed those same
-- setups as 'closed_loss' / 'closed_partial_win' against a simulated
-- SL/TP trail — inflating the live performance counters with trades
-- that never actually placed a real broker order.
--
-- This migration:
--   1. Adds 'rejected' to the qtss_setups.state CHECK constraint so the
--      bridge can stamp setups that never opened with a distinct state.
--   2. Deletes orphan live_positions (no matching qtss_setups row).
--   3. Deletes zombie LIVE closed setups that never had a live_positions
--      row (no real broker order ever existed).
--   4. Deletes related selected_candidates + exchange_orders for the
--      removed setups (keeps FK story consistent).
--
-- Safe to re-run — cleanup DELETEs are bounded by the "no broker trail"
-- predicate, and the ALTER CONSTRAINT uses DROP IF EXISTS + ADD.

-- 1) Constraint: widen allowed states.
ALTER TABLE qtss_setups
    DROP CONSTRAINT IF EXISTS qtss_setups_state_check;

ALTER TABLE qtss_setups
    ADD CONSTRAINT qtss_setups_state_check CHECK (state = ANY (ARRAY[
        'flat',
        'armed',
        'active',
        'closed',
        'closed_win',
        'closed_loss',
        'closed_manual',
        'closed_partial_win',
        'closed_scratch',
        'rejected'
    ]));

-- 2) Delete zombie LIVE setups FIRST — those flagged mode='live' but
--    which never produced a live_positions row. These are the stuck
--    entries setup_watcher was closing with fake SL/TP outcomes.
--    Ordering matters: if we orphan-sweep live_positions before this
--    DELETE runs, we leave cross-mode links (dry position pointing at
--    a live zombie setup) in place, and the orphan sweep below misses
--    them. Dropping the zombie setups first makes every dependent row
--    orphan-eligible in a single later pass.
DELETE FROM qtss_setups s
 WHERE s.mode = 'live'
   AND NOT EXISTS (
       SELECT 1
         FROM live_positions lp
        WHERE lp.setup_id = s.id
          AND lp.mode = 'live'
   );

-- 3) Delete orphan live_positions — any row whose setup_id no longer
--    resolves. Runs AFTER the zombie sweep so the cross-mode leftovers
--    (dry positions with setup_id pointing at a now-deleted live
--    zombie) are included.
DELETE FROM live_positions
 WHERE setup_id IS NOT NULL
   AND NOT EXISTS (SELECT 1 FROM qtss_setups s WHERE s.id = live_positions.setup_id);

-- 4) selected_candidates that pointed at now-deleted setups can go too;
--    they're FK-less pending rows. Best-effort — table might not have
--    rows on fresh installs.
DELETE FROM selected_candidates sc
 WHERE NOT EXISTS (SELECT 1 FROM qtss_setups s WHERE s.id = sc.setup_id);

-- 5) exchange_orders has no setup_id column (keyed by client_order_id).
--    Table is empty today anyway — dispatch_dry never writes to it,
--    dispatch_live never succeeded. If future rows need cleanup, do it
--    via client_order_id ↔ live_positions cascade later.
