-- Canonical pivot store — single source of truth for all zigzag data
-- across worker write path and GUI read path. Replaces the ad-hoc
-- `pivot_cache` / `pivot_backfill_state` paths used by the old
-- `qtss-pivots` library zigzag (trailing `ta.highestbars`).
--
-- The zigzag algorithm is now LuxAlgo's centered `ta.pivothigh(hi, left, 1)`
-- (1:1 Pine port in `crates/qtss-pivots/src/zigzag.rs`), with five
-- configurable slots (L0..L4, Fibonacci defaults 3/5/8/13/21).
--
-- `swing_tag` records the HH/HL/LL/LH classification relative to the
-- most recent same-direction predecessor — display-only; does NOT
-- feed back into the detection pipeline.

CREATE TABLE IF NOT EXISTS pivots (
    id                UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id  UUID        NOT NULL REFERENCES engine_symbols(id) ON DELETE CASCADE,
    level             SMALLINT    NOT NULL CHECK (level >= 0 AND level <= 4),
    bar_index         BIGINT      NOT NULL,
    open_time         TIMESTAMPTZ NOT NULL,
    direction         SMALLINT    NOT NULL CHECK (direction IN (-1, 1)),
    price             NUMERIC     NOT NULL,
    volume            NUMERIC     NOT NULL DEFAULT 0,
    swing_tag         TEXT        CHECK (swing_tag IN ('HH','HL','LL','LH')),
    computed_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (engine_symbol_id, level, open_time)
);

CREATE INDEX IF NOT EXISTS pivots_symbol_level_time_idx
    ON pivots (engine_symbol_id, level, open_time DESC);

CREATE INDEX IF NOT EXISTS pivots_symbol_level_bar_idx
    ON pivots (engine_symbol_id, level, bar_index);

COMMENT ON TABLE  pivots IS 'Canonical zigzag pivots (LuxAlgo centered). One row per pivot per level per series.';
COMMENT ON COLUMN pivots.level IS 'Zigzag slot 0..4 — length resolved from system_config zigzag.slot_<n>.';
COMMENT ON COLUMN pivots.direction IS '+1 = high pivot, -1 = low pivot.';
COMMENT ON COLUMN pivots.swing_tag IS 'HH/HL/LL/LH classification vs prior same-direction pivot. NULL when no predecessor.';
