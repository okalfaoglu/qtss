-- Historical outcome tracker (PR-13D).
--
-- For every detection we back-test the pattern forward from its
-- `start_time` bar by bar. For each bar we check: did price touch any
-- TP? did it touch SL? did the time-expiry cap fire? The first of
-- these wins and becomes the detection's outcome.
--
-- AI / ML / RADAR reports consume this table — "pattern_family=
-- harmonic, subkind=gartley_bull, TF=4h" → hit rate, avg MFE/MAE,
-- median bars-to-outcome — so the multi-gate AI approval's
-- `meta_label` probability has ground-truth labels to train on.
--
-- This is different from `validator_loop` which only answers "is this
-- still actionable RIGHT NOW". Outcome tracker answers "at the time
-- this pattern was active, what actually happened?".

CREATE TABLE IF NOT EXISTS pattern_outcomes (
    -- Composite detection key matches the upsert key in `detections`.
    exchange        TEXT        NOT NULL,
    segment         TEXT        NOT NULL,
    symbol          TEXT        NOT NULL,
    timeframe       TEXT        NOT NULL,
    slot            SMALLINT    NOT NULL,
    pattern_family  TEXT        NOT NULL,
    subkind         TEXT        NOT NULL,
    start_time      TIMESTAMPTZ NOT NULL,
    mode            TEXT        NOT NULL,

    -- Outcome classification. One of:
    --   'tp1_hit', 'tp2_hit', 'tp3_hit' — price reached a TP first
    --   'sl_hit'    — price reached SL first
    --   'expired'   — time-expiry cap fired without TP/SL
    --   'active'    — still within evaluation window, no outcome yet
    --   'invalidated' — structural break (validator_loop flipped it)
    outcome         TEXT        NOT NULL,
    -- Highest TP reached (0 = none, 1/2/3 = TP ordinal).
    tp_hit_count    SMALLINT    NOT NULL DEFAULT 0,
    -- Bar index from `start_time` at which the outcome fired.
    bars_to_outcome INT,
    outcome_time    TIMESTAMPTZ,
    outcome_price   DOUBLE PRECISION,
    -- MFE = max favorable price excursion from entry before outcome.
    -- MAE = max adverse excursion. Both signed in the trade's
    -- direction (positive = good for bulls, negative = bad for bulls).
    mfe             DOUBLE PRECISION,
    mae             DOUBLE PRECISION,
    -- The entry + TP/SL levels the tracker used, for audit. Sourced
    -- from the detection's primary TargetSource (or ATR fallback).
    target_json     JSONB,
    evaluated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (exchange, segment, symbol, timeframe, slot,
                 pattern_family, subkind, start_time, mode)
);

CREATE INDEX IF NOT EXISTS pattern_outcomes_by_family
    ON pattern_outcomes (pattern_family, subkind, timeframe, evaluated_at DESC);

CREATE INDEX IF NOT EXISTS pattern_outcomes_by_symbol
    ON pattern_outcomes (symbol, timeframe, evaluated_at DESC);

CREATE INDEX IF NOT EXISTS pattern_outcomes_active_idx
    ON pattern_outcomes (evaluated_at)
    WHERE outcome = 'active';

COMMENT ON TABLE pattern_outcomes IS
    'Per-detection historical outcome: did the pattern hit TP or SL first, when, and with what MFE/MAE? Feeds ML meta-label training, RADAR performance reports, and per-family hit-rate dashboards.';

-- Config seeds for the outcome-tracker worker loop.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('outcome_tracker', 'enabled',        '{"enabled": true}'::jsonb,
     'Master on/off for the historical outcome-tracker worker loop.'),
    ('outcome_tracker', 'tick_secs',      '{"secs": 300}'::jsonb,
     'Loop cadence (seconds). Evaluating forward is cheap but the replay is expensive on large windows — every 5 minutes keeps the active set fresh without overwhelming the DB.'),
    ('outcome_tracker', 'batch_size',     '{"value": 500}'::jsonb,
     'Max detections re-evaluated per tick. Order: active outcomes first (still evolving), then freshly-inserted rows without a row in pattern_outcomes.'),

    -- Per-family time-expiry (bars). Beyond this the outcome is
    -- marked `expired` regardless of price.
    ('outcome_tracker', 'expiry.harmonic',  '{"bars": 200}'::jsonb,
     'Harmonic pattern time-expiry (bars). After N bars without hitting TP/SL, outcome = expired.'),
    ('outcome_tracker', 'expiry.classical', '{"bars": 120}'::jsonb, 'Classical pattern time-expiry.'),
    ('outcome_tracker', 'expiry.range',     '{"bars": 100}'::jsonb, 'Range zone time-expiry.'),
    ('outcome_tracker', 'expiry.motive',    '{"bars": 500}'::jsonb, 'Elliott motive time-expiry (macro).'),
    ('outcome_tracker', 'expiry.abc',       '{"bars": 300}'::jsonb, 'Elliott ABC time-expiry.'),
    ('outcome_tracker', 'expiry.candle',    '{"bars": 5}'::jsonb,   'Candlestick pattern time-expiry (micro).'),
    ('outcome_tracker', 'expiry.gap',       '{"bars": 50}'::jsonb,  'Gap pattern time-expiry.'),
    ('outcome_tracker', 'expiry.orb',       '{"bars": 48}'::jsonb,  'ORB time-expiry (session-bound).'),
    ('outcome_tracker', 'expiry.smc',       '{"bars": 50}'::jsonb,  'SMC event time-expiry.'),

    -- ATR fallback parameters for outcomes without a target-resolver
    -- hit (rare — most detections carry invalidation_price or
    -- harmonic anchors).
    ('outcome_tracker', 'fallback.atr_tp_mult', '{"value": 2.0}'::jsonb,
     'Fallback TP multiplier in ATR units when no TargetResolver matches.'),
    ('outcome_tracker', 'fallback.atr_sl_mult', '{"value": 1.0}'::jsonb,
     'Fallback SL multiplier in ATR units.')
ON CONFLICT (module, config_key) DO NOTHING;
