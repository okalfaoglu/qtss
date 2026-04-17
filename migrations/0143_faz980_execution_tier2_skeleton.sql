-- Faz 9.8.0 — Tier-2 execution skeleton.
--
-- Three new tables + config keys for the tick-driven execution
-- lifecycle. These are the backing store for qtss-risk's new
-- post-trade modules (live_position_store, liquidation_guard,
-- scale_manager). Dry and live modes share schema — the `mode`
-- column discriminates.

-- -----------------------------------------------------------------
-- 1) live_positions — broker-filled positions being tracked tick-
--    by-tick. One row per (exchange, symbol, side, mode) open
--    position. Closed positions are retained for attribution
--    (closed_at IS NOT NULL).
-- -----------------------------------------------------------------
CREATE TABLE IF NOT EXISTS live_positions (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id                UUID NOT NULL,
    user_id               UUID NOT NULL,
    setup_id              UUID,                                -- qtss_v2_setups.id (nullable: manual opens)
    mode                  TEXT NOT NULL CHECK (mode IN ('dry','live')),
    exchange              TEXT NOT NULL,
    segment               TEXT NOT NULL,                       -- spot|futures|margin|options
    symbol                TEXT NOT NULL,
    side                  TEXT NOT NULL CHECK (side IN ('BUY','SELL')),
    leverage              SMALLINT NOT NULL DEFAULT 1,
    entry_avg             NUMERIC(38,18) NOT NULL,
    qty_filled            NUMERIC(38,18) NOT NULL,
    qty_remaining         NUMERIC(38,18) NOT NULL,             -- after scale-out
    current_sl            NUMERIC(38,18),
    tp_ladder             JSONB NOT NULL DEFAULT '[]'::jsonb,  -- [{price, qty, filled_qty}]
    liquidation_price     NUMERIC(38,18),                      -- venue-provided, tick-validated
    maint_margin_ratio    NUMERIC(10,6),                       -- e.g. 0.004 = 0.4%
    funding_rate_next     NUMERIC(10,6),
    last_mark             NUMERIC(38,18),
    last_tick_at          TIMESTAMPTZ,
    opened_at             TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    closed_at             TIMESTAMPTZ,
    close_reason          TEXT,                                -- tp_final|sl_hit|liquidation_guard|manual|...
    realized_pnl_quote    NUMERIC(38,18),
    metadata              JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_live_positions_open
    ON live_positions (mode, exchange, symbol) WHERE closed_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_live_positions_setup
    ON live_positions (setup_id) WHERE setup_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_live_positions_user
    ON live_positions (user_id, opened_at DESC);

COMMENT ON TABLE live_positions IS
    'Faz 9.8 Tier-2 execution: broker-filled positions tracked tick-by-tick. Mode=dry|live.';

-- -----------------------------------------------------------------
-- 2) position_scale_events — scale-in (pyramid) and scale-out
--    (partial close, add-on-dip) history. One row per event.
-- -----------------------------------------------------------------
CREATE TABLE IF NOT EXISTS position_scale_events (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    position_id           UUID NOT NULL REFERENCES live_positions(id) ON DELETE CASCADE,
    event_kind            TEXT NOT NULL CHECK (event_kind IN
                              ('scale_in','scale_out','add_on_dip','partial_tp','ratchet_sl')),
    price                 NUMERIC(38,18) NOT NULL,
    qty_delta             NUMERIC(38,18) NOT NULL,             -- +add / -remove
    qty_after             NUMERIC(38,18) NOT NULL,
    entry_avg_after       NUMERIC(38,18) NOT NULL,
    realized_pnl_quote    NUMERIC(38,18),                      -- for scale_out/partial_tp
    reason                TEXT,
    metadata              JSONB NOT NULL DEFAULT '{}'::jsonb,
    occurred_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_scale_events_position
    ON position_scale_events (position_id, occurred_at DESC);

COMMENT ON TABLE position_scale_events IS
    'Faz 9.8: pyramid-in, scale-out, add-on-dip, partial TP, ratchet SL events.';

-- -----------------------------------------------------------------
-- 3) liquidation_guard_events — alerts + auto-actions triggered by
--    the liquidation guard (margin ratio breach, auto add-margin,
--    panic close).
-- -----------------------------------------------------------------
CREATE TABLE IF NOT EXISTS liquidation_guard_events (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    position_id           UUID NOT NULL REFERENCES live_positions(id) ON DELETE CASCADE,
    severity              TEXT NOT NULL CHECK (severity IN ('warn','high','breach')),
    action_taken          TEXT NOT NULL CHECK (action_taken IN
                              ('none','alert','add_margin','scale_out','panic_close')),
    mark_price            NUMERIC(38,18) NOT NULL,
    liquidation_price     NUMERIC(38,18) NOT NULL,
    distance_pct          NUMERIC(10,6) NOT NULL,              -- (liq - mark)/mark, signed
    margin_ratio          NUMERIC(10,6),
    details               JSONB NOT NULL DEFAULT '{}'::jsonb,
    occurred_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_liq_guard_position
    ON liquidation_guard_events (position_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_liq_guard_severity
    ON liquidation_guard_events (severity, occurred_at DESC);

COMMENT ON TABLE liquidation_guard_events IS
    'Faz 9.8: liquidation guard alerts + auto-actions (add-margin, panic-close).';

-- -----------------------------------------------------------------
-- 4) Config keys — module gates + thresholds. All tunable via
--    Config Editor; defaults err on the conservative side.
-- -----------------------------------------------------------------

-- Master gates
SELECT _qtss_register_key(
    'execution.tier2_enabled', 'execution', 'tier2',
    'bool', 'false'::jsonb, '',
    'Master switch for Faz 9.8 Tier-2 execution (setup selector + auto-open).',
    'bool', true, 'high', ARRAY['execution','faz98','tier2']
);

SELECT _qtss_register_key(
    'execution.mode', 'execution', 'tier2',
    'string', '"dry"'::jsonb, '',
    'Execution mode: dry (paper) or live (broker). Dry + live can run in parallel via separate workers.',
    'string', true, 'high', ARRAY['execution','faz98','tier2']
);

-- Liquidation guard thresholds
SELECT _qtss_register_key(
    'risk.liquidation_guard.enabled', 'risk', 'liquidation_guard',
    'bool', 'true'::jsonb, '',
    'Enable liquidation guard tick-level monitoring.',
    'bool', true, 'high', ARRAY['risk','faz98','liquidation']
);

SELECT _qtss_register_key(
    'risk.liquidation_guard.warn_distance_pct', 'risk', 'liquidation_guard',
    'float', '0.08'::jsonb, '',
    'Warn when (liq - mark)/mark crosses this threshold (default 8%).',
    'number', false, 'normal', ARRAY['risk','faz98','liquidation']
);

SELECT _qtss_register_key(
    'risk.liquidation_guard.critical_distance_pct', 'risk', 'liquidation_guard',
    'float', '0.04'::jsonb, '',
    'Critical alert threshold — triggers auto add-margin or scale-out.',
    'number', false, 'high', ARRAY['risk','faz98','liquidation']
);

SELECT _qtss_register_key(
    'risk.liquidation_guard.panic_close_distance_pct', 'risk', 'liquidation_guard',
    'float', '0.015'::jsonb, '',
    'Panic-close threshold — market-out immediately to avoid liquidation.',
    'number', false, 'high', ARRAY['risk','faz98','liquidation']
);

SELECT _qtss_register_key(
    'risk.liquidation_guard.cooldown_minutes', 'risk', 'liquidation_guard',
    'int', '30'::jsonb, 'min',
    'After a liquidation-triggered close, block new positions on the same symbol for N minutes.',
    'number', false, 'normal', ARRAY['risk','faz98','liquidation']
);

-- Scale manager
SELECT _qtss_register_key(
    'risk.scale_manager.enabled', 'risk', 'scale_manager',
    'bool', 'true'::jsonb, '',
    'Enable pyramid-in / scale-out / add-on-dip.',
    'bool', true, 'normal', ARRAY['risk','faz98','scale']
);

SELECT _qtss_register_key(
    'risk.scale_manager.max_pyramid_legs', 'risk', 'scale_manager',
    'int', '2'::jsonb, '',
    'Maximum pyramid-in legs per position (0 disables pyramid).',
    'number', false, 'normal', ARRAY['risk','faz98','scale']
);

-- Live position store tick cadence
SELECT _qtss_register_key(
    'risk.live_position_store.tick_tick_secs', 'risk', 'live_position_store',
    'int', '2'::jsonb, 's',
    'Fallback eval cadence when tick stream is quiet (seconds).',
    'number', false, 'normal', ARRAY['risk','faz98','tick']
);

-- Slippage guard
SELECT _qtss_register_key(
    'execution.slippage_guard.max_bps', 'execution', 'slippage_guard',
    'int', '25'::jsonb, 'bps',
    'Max acceptable slippage in basis points at fill vs intent price.',
    'number', false, 'normal', ARRAY['execution','faz98','slippage']
);

SELECT _qtss_register_key(
    'execution.slippage_guard.limit_wait_secs', 'execution', 'slippage_guard',
    'int', '30'::jsonb, 's',
    'Cancel (or market-upgrade) a limit order if not filled within N seconds.',
    'number', false, 'normal', ARRAY['execution','faz98','slippage']
);

-- Funding rate monitoring
SELECT _qtss_register_key(
    'risk.funding_guard.enabled', 'risk', 'funding_guard',
    'bool', 'false'::jsonb, '',
    'Reduce or skip positions before unfavorable funding payments.',
    'bool', false, 'normal', ARRAY['risk','faz98','funding']
);

SELECT _qtss_register_key(
    'risk.funding_guard.max_cost_pct', 'risk', 'funding_guard',
    'float', '0.01'::jsonb, '',
    'Skip entry if next funding cost exceeds this pct of notional.',
    'number', false, 'normal', ARRAY['risk','faz98','funding']
);
