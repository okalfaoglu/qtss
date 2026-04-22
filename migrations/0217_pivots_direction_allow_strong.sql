-- Relax pivots.direction CHECK so the Pine-port zigzag's dir*2
-- "strong trend" marker (HH / LL continuation) can land in the
-- column unchanged.
--
-- The old constraint `direction IN (-1, 1)` dates from before the
-- Pine port replaced the trailing-window zigzag. After that port
-- pivot_writer_loop started emitting ±2 for pivots that strictly beat
-- the previous same-direction pivot — these got rejected wholesale
-- by the CHECK, which meant pivot_writer produced zero rows per
-- tick (see harmonic/Elliott DB-vs-live parity bug).
--
-- Downstream readers that only care about the sign use .signum() or
-- match on pattern direction already, so widening to ±2 is
-- backward-compatible. Any consumer that wants strict-±1 semantics
-- can call direction.signum() post-read.

ALTER TABLE pivots DROP CONSTRAINT IF EXISTS pivots_direction_check;
ALTER TABLE pivots ADD CONSTRAINT pivots_direction_check
    CHECK (direction = ANY(ARRAY[-2, -1, 1, 2]::integer[]));
