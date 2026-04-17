-- Faz 9.7.0 — Setup lifecycle events + extensions to qtss_v2_setups
--
-- Every tick the SetupWatcher compares current price against each
-- active setup's key levels. When a boundary is crossed (entry touch,
-- TP hit, SL hit, structural invalidation, manual cancel) a lifecycle
-- event is emitted and persisted. The `notify_outbox` / `x_outbox`
-- loops consume these events and render user-facing cards.
--
-- NOTE: There is NO time-based TTL (decision locked by user —
-- "setuplar hedef odaklıdır. o hedef gelmeden iptal olmamalıdır").
-- Closures happen only via tp_final, sl_hit, invalidated, cancelled.

-- 1. Extend qtss_v2_setups with closure + protection state.
ALTER TABLE qtss_v2_setups
    ADD COLUMN IF NOT EXISTS entry_touched_at   TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS closed_at          TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS close_reason       TEXT,       -- tp_final | sl_hit | invalidated | cancelled
    ADD COLUMN IF NOT EXISTS close_price        NUMERIC,
    ADD COLUMN IF NOT EXISTS realized_pnl_pct   NUMERIC,    -- signed % from entry
    ADD COLUMN IF NOT EXISTS realized_r         NUMERIC,    -- multiples of initial risk
    -- Partial TP accounting.
    ADD COLUMN IF NOT EXISTS tp_hits_bitmap     INTEGER NOT NULL DEFAULT 0,  -- bit0=TP1, bit1=TP2, bit2=TP3, ...
    ADD COLUMN IF NOT EXISTS remaining_qty_pct  NUMERIC NOT NULL DEFAULT 100.0,
    -- Poz Koruma (Profit Ratchet) state.
    ADD COLUMN IF NOT EXISTS current_sl         NUMERIC,    -- live SL (may be higher than original if ratchet fired)
    ADD COLUMN IF NOT EXISTS ratchet_reference_price  NUMERIC,  -- last EOD close used for daily gain calc
    ADD COLUMN IF NOT EXISTS ratchet_last_update_at   TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS ratchet_cumulative_pct   NUMERIC NOT NULL DEFAULT 0.0;

COMMENT ON COLUMN qtss_v2_setups.ratchet_cumulative_pct IS
  'Poz Koruma: cumulative daily gain %% locked into SL. SL = entry * (1 + this/100) for LONG.';

-- Normalize legacy close_reason values to the new canonical vocabulary.
-- Legacy values (from the v2 setup loop pre-Faz-9.7) are remapped so
-- the CHECK constraint below can be applied cleanly.
UPDATE qtss_v2_setups SET close_reason = 'tp_final'
    WHERE close_reason IN ('target_hit');
UPDATE qtss_v2_setups SET close_reason = 'sl_hit'
    WHERE close_reason IN ('stop_hit');
UPDATE qtss_v2_setups SET close_reason = 'invalidated'
    WHERE close_reason IN ('p14_opposite_dir_conflict','reverse_signal');

-- Ensure close_reason is from the canonical set when set.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'qtss_v2_setups_close_reason_chk'
    ) THEN
        ALTER TABLE qtss_v2_setups
            ADD CONSTRAINT qtss_v2_setups_close_reason_chk
            CHECK (close_reason IS NULL OR close_reason IN
                ('tp_final','sl_hit','invalidated','cancelled'));
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_qtss_v2_setups_active_watcher
    ON qtss_v2_setups (exchange, symbol, timeframe)
    WHERE closed_at IS NULL;

-- 2. Lifecycle events table — audit trail per setup.
CREATE TABLE IF NOT EXISTS qtss_setup_lifecycle_events (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    setup_id            UUID NOT NULL REFERENCES qtss_v2_setups(id) ON DELETE CASCADE,
    event_kind          TEXT NOT NULL,  -- see CHECK below
    price               NUMERIC NOT NULL,
    -- Context at event time.
    pnl_pct             NUMERIC,
    pnl_r               NUMERIC,
    health_score        NUMERIC,
    duration_ms         BIGINT,         -- from setup open
    -- Smart Target AI decision (filled only on tp_hit events that invoke AI).
    ai_action           TEXT,           -- ride | scale | exit | tighten | null
    ai_reasoning        TEXT,
    ai_confidence       NUMERIC,
    -- Notify pipeline back-refs.
    notify_outbox_id    UUID,
    x_outbox_id         UUID,
    emitted_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT qtss_setup_lifecycle_events_kind_chk
        CHECK (event_kind IN (
            'entry_touched',
            'tp_hit',             -- any TP level (detail in payload or ai_action)
            'tp_partial',         -- scale-out executed
            'tp_final',           -- full exit at target
            'sl_hit',
            'sl_ratcheted',       -- Poz Koruma moved SL up
            'invalidated',        -- structural break
            'cancelled',          -- manual / regime
            'health_warn',        -- health dropped into warn band
            'health_danger'       -- health dropped into danger band
        )),
    CONSTRAINT qtss_setup_lifecycle_events_ai_action_chk
        CHECK (ai_action IS NULL OR ai_action IN ('ride','scale','exit','tighten'))
);

CREATE INDEX IF NOT EXISTS idx_lifecycle_events_setup
    ON qtss_setup_lifecycle_events (setup_id, emitted_at DESC);
CREATE INDEX IF NOT EXISTS idx_lifecycle_events_kind_time
    ON qtss_setup_lifecycle_events (event_kind, emitted_at DESC);

-- 3. Position health snapshots — persisted only at threshold crossings.
CREATE TABLE IF NOT EXISTS qtss_position_health_snapshots (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    setup_id            UUID NOT NULL REFERENCES qtss_v2_setups(id) ON DELETE CASCADE,
    health_score        NUMERIC NOT NULL,    -- 0..100
    prev_health_score   NUMERIC,
    band                TEXT NOT NULL,       -- healthy | warn | danger | critical
    prev_band           TEXT,
    momentum_score      NUMERIC,
    structural_score    NUMERIC,
    orderbook_score     NUMERIC,
    regime_match_score  NUMERIC,
    correlation_score   NUMERIC,
    ai_rescore          NUMERIC,
    price               NUMERIC NOT NULL,
    captured_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_position_health_setup
    ON qtss_position_health_snapshots (setup_id, captured_at DESC);

COMMENT ON TABLE qtss_position_health_snapshots IS
  'Position Health Score persisted at band transitions only (healthy<->warn<->danger<->critical). Tick-level computation stays in memory.';
