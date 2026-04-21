-- Add prominence to the canonical `pivots` table.
--
-- prominence = |price - prior_confirmed_pivot_price|, computed by
-- `ZigZag::push` in `crates/qtss-pivots/src/zigzag.rs`. The pivot-reversal
-- detector (crates/qtss-pivot-reversal/src/lib.rs:282, 333-337) uses it
-- in two places:
--   * floor filter  — pivots below `prominence_floor(level)` are skipped
--   * setup score   — 30% weight in final score (`tier*0.7 + prom*0.3`)
-- The 5 backtest sweeps read it too. Without it, setup scoring loses
-- magnitude-awareness and all reversals look identical.
--
-- The `pivot_writer_loop` already produces prominence in its
-- `ConfirmedPivot` output; this patch (0208 + worker change) persists it.

ALTER TABLE pivots
    ADD COLUMN IF NOT EXISTS prominence NUMERIC NOT NULL DEFAULT 0;

COMMENT ON COLUMN pivots.prominence IS '|price - prior_same-series_confirmed_pivot_price|. 0 when this is the first pivot for the (engine_symbol_id, level) pair.';
