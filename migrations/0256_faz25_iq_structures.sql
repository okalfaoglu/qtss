-- FAZ 25 PR-25B — IQ-D structure tracker tables.
--
-- Each row in `iq_structures` represents a single Elliott structure
-- (impulse + ABC = full 8-wave cycle) being followed for a specific
-- (exchange, segment, symbol, timeframe, slot) tuple. The tracker
-- worker (qtss-worker/src/iq_structure_tracker_loop.rs) creates a
-- new row when a fresh impulse is detected, transitions it through
-- the wave states, and marks it `invalidated` when Elliott rules are
-- broken (W2 retraces > 100%, W4-W1 overlap, W3 not the shortest of
-- W1/W3/W5, ABC complete then no new W1 in time, …).
--
-- A separate `iq_symbol_locks` table records the "no new trades on
-- this symbol until a fresh IQ-D candidate appears" rule. The
-- allocator (PR-25E) consults this lock before evaluating any IQ-D
-- or IQ-T candidate.
--
-- Strict-isolation principle (FAZ 25 §0): the existing T/D allocator
-- profiles and `qtss_setups` writes are NOT affected. These tables
-- are new, the tracker is a new worker loop, the allocator profile
-- is added separately in PR-25E. Zero regression risk.

-- ── iq_structures ───────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS iq_structures (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange        text NOT NULL,
    segment         text NOT NULL,
    symbol          text NOT NULL,
    timeframe       text NOT NULL,
    slot            smallint NOT NULL,           -- 0..4 = Z1..Z5
    direction       smallint NOT NULL,           -- +1 bull, -1 bear
    -- Lifecycle: candidate → tracking → completed → invalidated.
    -- 'completed' = full impulse + ABC done. 'invalidated' = an
    -- Elliott rule was broken; lock the symbol until a new candidate
    -- arrives.
    state           text NOT NULL DEFAULT 'candidate',
    -- Current wave being tracked: W1, W2, W3, W4, W5, A, B, C.
    current_wave    text NOT NULL DEFAULT 'W1',
    -- Nascent / forming / completed for the current wave.
    current_stage   text NOT NULL DEFAULT 'forming',
    -- Pivot anchors collected so far (chronological JSON array of
    -- {bar_index, time, price, direction, wave_label}).
    structure_anchors jsonb NOT NULL DEFAULT '[]'::jsonb,
    -- Diagnostic hash so the tracker can detect "same impulse, more
    -- pivots filled in" vs "completely different structure". Hash is
    -- of the first three pivots' rounded prices + the slot.
    seed_hash       text NOT NULL,
    started_at      timestamptz NOT NULL DEFAULT now(),
    last_advanced_at timestamptz NOT NULL DEFAULT now(),
    completed_at    timestamptz,
    invalidated_at  timestamptz,
    invalidation_reason text,
    -- Side-channel for analytics / GUI overlays.
    raw_meta        jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now()
);

-- One ACTIVE structure per (symbol, tf, slot) at a time. Older
-- invalidated/completed rows stay for audit. Partial-unique index
-- keyed on the live-only states.
CREATE UNIQUE INDEX IF NOT EXISTS iq_structures_active_uniq
    ON iq_structures (exchange, segment, symbol, timeframe, slot)
    WHERE state IN ('candidate','tracking');

CREATE INDEX IF NOT EXISTS iq_structures_recent_idx
    ON iq_structures (symbol, timeframe, slot, started_at DESC);

CREATE INDEX IF NOT EXISTS iq_structures_seed_idx
    ON iq_structures (seed_hash);

-- ── iq_symbol_locks ─────────────────────────────────────────────────
-- "Yapı bozulduktan sonra o sembolde yeni IQ-D candidate gelene kadar
-- yeni işlem açma" kuralının materializasyonu. Allocator PR-25E'de
-- bu tabloyu kontrol edecek.
CREATE TABLE IF NOT EXISTS iq_symbol_locks (
    exchange        text NOT NULL,
    segment         text NOT NULL,
    symbol          text NOT NULL,
    locked_at       timestamptz NOT NULL DEFAULT now(),
    last_invalidated_id uuid REFERENCES iq_structures(id),
    reason          text,
    -- A lock auto-clears when a NEW iq_structures row in 'candidate'
    -- state appears for the same symbol. The tracker handles that
    -- DELETE side; allocator only reads.
    created_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (exchange, segment, symbol)
);

CREATE INDEX IF NOT EXISTS iq_symbol_locks_locked_at_idx
    ON iq_symbol_locks (locked_at DESC);

-- ── config seeds ────────────────────────────────────────────────────
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('iq_structure', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master switch for the IQ structure tracker loop. When false the loop sleeps an hour and skips work.'),
    ('iq_structure', 'tick_secs',
     '{"secs": 90}'::jsonb,
     'Tracker cadence in seconds. Default 90s ~ 1.5x the engine writer tick so we always see the freshest motive + early-wave rows. Min clamped to 30s.'),
    ('iq_structure', 'lookback_bars',
     '{"value": 1000}'::jsonb,
     'How many recent OHLC bars to consider when matching detection rows back to the current chart window. Mirrors the elliott writer''s default.'),
    ('iq_structure', 'invalidation_tol_pct',
     '{"value": 0.005}'::jsonb,
     'Price tolerance for the W2/W4 overlap and W5-break invalidation rules (0.005 = 0.5%). Same value used by elliott_early to avoid wick noise tripping the count.'),
    ('iq_structure', 'symbol_lock_min_age_secs',
     '{"value": 60}'::jsonb,
     'Lock a symbol for at LEAST this many seconds after invalidation, even if a new candidate appears. Prevents lock thrash from a noisy pivot tape.')
ON CONFLICT (module, config_key) DO NOTHING;
