-- Faz 9.8.11 — perf indexes + selection→execution wiring tables.
--
-- Two concerns bundled because they share a migration slot:
--
-- 1. Slow-query fixes reported from prod logs
--    - qtss_features_snapshot: GROUP BY / LATERAL jsonb_object_keys paths
--      were seq-scanning. 19s → sub-second with (source, computed_at).
--    - qtss_v2_detections DISTINCT ON path was sampling 130s — partial
--      index keyed on the exact WHERE + ORDER BY shape.
--
-- 2. `selected_candidates` table — output of the selector worker loop
--    (Faz 9.8.11). Bridges setup → execution: the selector writes one
--    row per approved candidate, execution worker consumes FOR UPDATE
--    SKIP LOCKED and dispatches to ExecutionManager.

-- ---------------------------------------------------------------------
-- Perf indexes
-- ---------------------------------------------------------------------

CREATE INDEX IF NOT EXISTS idx_features_snap_source_time
    ON qtss_features_snapshot (source, computed_at DESC);

CREATE INDEX IF NOT EXISTS idx_features_snap_det_time
    ON qtss_features_snapshot (detection_id, computed_at DESC);

-- Partial index matches the exact DISTINCT ON (family, subkind) query
-- in qtss-api routes/v2_detections.rs. State predicate excludes dead rows.
CREATE INDEX IF NOT EXISTS idx_v2_det_family_latest
    ON qtss_v2_detections (exchange, symbol, timeframe, family, subkind, detected_at DESC)
    WHERE state <> 'invalidated';

-- ---------------------------------------------------------------------
-- selected_candidates — selector worker → execution bridge
-- ---------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS selected_candidates (
    id              BIGSERIAL PRIMARY KEY,
    setup_id        UUID        NOT NULL,
    exchange        TEXT        NOT NULL,
    symbol          TEXT        NOT NULL,
    timeframe       TEXT        NOT NULL,
    direction       TEXT        NOT NULL CHECK (direction IN ('long','short')),
    entry_price     NUMERIC     NOT NULL,
    sl_price        NUMERIC     NOT NULL,
    tp_ladder       JSONB       NOT NULL DEFAULT '[]'::jsonb,
    risk_pct        NUMERIC     NOT NULL,
    mode            TEXT        NOT NULL CHECK (mode IN ('dry','live','backtest')),
    -- Lifecycle: 'pending' → execution worker picks up; 'claimed' while
    -- dispatched; 'placed' after broker ack; 'rejected' on risk veto;
    -- 'errored' on unexpected failure.
    status          TEXT        NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending','claimed','placed','rejected','errored')),
    reject_reason   TEXT,
    attempts        INTEGER     NOT NULL DEFAULT 0,
    last_error      TEXT,
    selector_score  NUMERIC,
    selector_meta   JSONB       NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    claimed_at      TIMESTAMPTZ,
    placed_at       TIMESTAMPTZ,
    UNIQUE (setup_id, mode)
);

CREATE INDEX IF NOT EXISTS idx_selected_candidates_pending
    ON selected_candidates (created_at)
    WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_selected_candidates_setup
    ON selected_candidates (setup_id);

COMMENT ON TABLE selected_candidates IS
    'Faz 9.8.11 — selector worker output; execution worker consumes via FOR UPDATE SKIP LOCKED';

-- ---------------------------------------------------------------------
-- Config keys for selector + execution loops
-- ---------------------------------------------------------------------

SELECT _qtss_register_key(
    'selector.loop_interval_ms', 'risk', 'selector',
    'int', '5000'::jsonb, '',
    'Selector worker tick interval (ms).',
    'number', false, 'normal', ARRAY['risk','faz9811','selector']
);

SELECT _qtss_register_key(
    'selector.batch_size', 'risk', 'selector',
    'int', '20'::jsonb, '',
    'Max setups processed per selector tick.',
    'number', false, 'normal', ARRAY['risk','faz9811','selector']
);

SELECT _qtss_register_key(
    'execution.dry.enabled', 'execution', 'dry',
    'bool', 'true'::jsonb, '',
    'Enable dry-mode execution worker (consumes selected_candidates).',
    'toggle', false, 'normal', ARRAY['execution','faz9811','dry']
);

SELECT _qtss_register_key(
    'execution.live.enabled', 'execution', 'live',
    'bool', 'false'::jsonb, '',
    'Enable live-mode execution worker (real broker orders).',
    'toggle', false, 'high', ARRAY['execution','faz9811','live']
);

SELECT _qtss_register_key(
    'execution.loop_interval_ms', 'execution', 'bridge',
    'int', '2000'::jsonb, '',
    'Execution bridge worker tick interval (ms).',
    'number', false, 'normal', ARRAY['execution','faz9811']
);
