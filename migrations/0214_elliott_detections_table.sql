-- `detections` — Elliott Wave pattern persistence, written by the
-- worker loop that mirrors the `/v2/elliott` endpoint. Each row is
-- one motive / ABC / triangle exactly as `luxalgo_pine_port::run`
-- emits it on the same pivot set that `pivot_writer_loop` already
-- persists to the `pivots` table.
--
-- This table lives alongside the legacy `qtss_v2_detections` rather
-- than replacing it: downstream consumers (AI layers, outcome
-- labeller, backtest performance) keep reading from the old path
-- while this one accumulates the richer PinePortOutput shape (Flat
-- + Triangle subkinds, break-box, next-marker, live flag).

CREATE TABLE IF NOT EXISTS detections (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    detected_at     timestamptz NOT NULL DEFAULT now(),
    exchange        text NOT NULL,
    segment         text NOT NULL DEFAULT 'futures',
    symbol          text NOT NULL,
    timeframe       text NOT NULL,

    -- Which Z-slot (0..4 = Z1..Z5) this pattern belongs to. Matches
    -- the `pivots.level` column one-to-one so the chart's per-slot
    -- filter reaches this table without a translation layer.
    slot            smallint NOT NULL,

    -- Pattern family: 'motive' | 'abc' | 'triangle'.
    -- Subkind disambiguates:
    --   motive    → 'impulse'
    --   abc       → 'zigzag' | 'flat_regular' | 'flat_expanded' | 'flat_running'
    --   triangle  → 'triangle_contracting' | 'triangle_expanding' | 'triangle_barrier'
    pattern_family  text NOT NULL,
    subkind         text NOT NULL,

    -- +1 bullish, -1 bearish. For triangles this is the parent-trend
    -- direction (triangle correcting a bull trend → direction=-1),
    -- matching PinePortOutput's convention.
    direction       smallint NOT NULL,

    -- Bar range the pattern spans. Paired with (exchange, segment,
    -- symbol, timeframe) these uniquely locate the pattern in the
    -- market_bars series — same bar_index basis the chart uses.
    start_bar       bigint NOT NULL,
    end_bar         bigint NOT NULL,
    start_time      timestamptz NOT NULL,
    end_time        timestamptz NOT NULL,

    -- Full anchor list as JSON — serialized PinePortOutput PivotPoint[].
    --   motive   = 6 anchors (p0..p5)
    --   abc      = 4 anchors (p5_of_parent, a, b, c)
    --   triangle = 6 anchors (p0..E)
    anchors         jsonb NOT NULL,

    -- Motive-only flags (NULL for abc / triangle rows).
    live            boolean,
    next_hint       boolean,

    -- Set true once the detector marks the pattern invalidated
    -- (ABC's C beyond parent's p5, etc.). Never for motives — live
    -- already captures that.
    invalidated     boolean NOT NULL DEFAULT false,

    -- Free-form metadata: fib band levels, break_box geometry,
    -- next_marker price, break_markers. Consumers tolerate missing
    -- keys. Example shape for a motive:
    --   { "break_box": {...}, "next_marker": {...}, "fib_band": {...} }
    raw_meta        jsonb NOT NULL DEFAULT '{}'::jsonb,

    mode            text NOT NULL DEFAULT 'live',
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),

    -- Idempotent upsert key: re-running the detector on the same
    -- (series, slot, family, subkind, span) must update in place.
    CONSTRAINT detections_unique_span
        UNIQUE (exchange, segment, symbol, timeframe, slot, pattern_family, subkind, start_bar, end_bar, mode)
);

CREATE INDEX IF NOT EXISTS detections_series_time_idx
    ON detections (exchange, segment, symbol, timeframe, detected_at DESC);

CREATE INDEX IF NOT EXISTS detections_family_subkind_idx
    ON detections (pattern_family, subkind);

CREATE INDEX IF NOT EXISTS detections_slot_idx
    ON detections (slot);

CREATE INDEX IF NOT EXISTS detections_mode_idx
    ON detections (mode) WHERE mode <> 'live';

-- Writer config. Default `enabled=true` — the user explicitly asked
-- for the detections table to be populated from the existing pivot
-- stream, so the loop is live out of the gate. Flip to false via
-- Config Editor to pause writes without redeploying.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detections', 'enabled',
        '{"enabled": true}'::jsonb,
        'Master switch for the detections writer loop. When true the worker mirrors /v2/elliott output into the detections table each tick.'),
    ('detections', 'tick_secs',
        '{"secs": 60}'::jsonb,
        'Polling interval in seconds. 60 matches pivot_writer_loop cadence so detections settle one tick after their source pivots are written.'),
    ('detections', 'bars_per_tick',
        '{"bars": 2000}'::jsonb,
        'Recent-bar window fed to luxalgo_pine_port::run each tick. Larger = more history but slower. 2000 matches pivot_writer.')
ON CONFLICT (module, config_key) DO NOTHING;
