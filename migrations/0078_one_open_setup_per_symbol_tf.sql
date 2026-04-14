-- 0078_one_open_setup_per_symbol_tf.sql — Faz 10 / P14.
--
-- Tighten the open-setup uniqueness from "one per (…, direction)" to
-- "one per (…)" so LONG and SHORT can never coexist as armed/open on
-- the same (exchange, symbol, timeframe, profile). App-level guard
-- in qtss-worker::v2_setup_loop::try_arm_new_setup is the first line
-- of defence; this index is the last.
--
-- Migration steps:
--   1. Close any currently-open opposite-direction pair by keeping
--      the one with the later `created_at` (most recent evidence)
--      and closing the other with a tagged reason.
--   2. Drop the old direction-aware unique index.
--   3. Create the stricter index without `direction`.

-- Step 1 — resolve existing conflicts (idempotent: no-op after first run).
WITH conflicts AS (
    SELECT exchange, symbol, timeframe, profile
      FROM qtss_v2_setups
     WHERE state IN ('armed', 'active')
     GROUP BY exchange, symbol, timeframe, profile
    HAVING count(DISTINCT direction) > 1
),
losers AS (
    SELECT s.id
      FROM qtss_v2_setups s
      JOIN conflicts c
        ON s.exchange = c.exchange
       AND s.symbol = c.symbol
       AND s.timeframe = c.timeframe
       AND s.profile = c.profile
     WHERE s.state IN ('armed', 'active')
       AND s.id NOT IN (
           SELECT DISTINCT ON (exchange, symbol, timeframe, profile) id
             FROM qtss_v2_setups
            WHERE state IN ('armed', 'active')
              AND (exchange, symbol, timeframe, profile) IN (
                  SELECT exchange, symbol, timeframe, profile FROM conflicts)
            ORDER BY exchange, symbol, timeframe, profile, created_at DESC
       )
)
UPDATE qtss_v2_setups
   SET state = 'closed',
       close_reason = COALESCE(close_reason, 'p14_opposite_dir_conflict')
 WHERE id IN (SELECT id FROM losers);

-- Step 2 — drop the old direction-aware unique index.
DROP INDEX IF EXISTS uq_open_setup_key;

-- Step 3 — one open setup per (exchange, symbol, timeframe, profile).
CREATE UNIQUE INDEX uq_open_setup_key
    ON qtss_v2_setups (exchange, symbol, timeframe, profile)
    WHERE state IN ('armed', 'active');
