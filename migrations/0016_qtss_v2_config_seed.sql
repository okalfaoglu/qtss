-- 0016_qtss_v2_config_seed.sql
--
-- Faz 0.5 — Default `config_schema` catalog seed.
--
-- This migration registers the *initial* set of configurable keys for QTSS v2.
-- It only seeds the schema catalog (`config_schema`) — overrides go into
-- `config_value` later, either via the GUI Config Editor or future migrations.
--
-- Per CLAUDE.md rule #2: anything tunable in code must live here. Adding a
-- new key in code path means adding it here in the same PR.
--
-- Default risk limits per architecture plan §10B. Conservative on purpose:
-- the operator will loosen them after live verification, never tighten them
-- in an emergency.

BEGIN;

-- ---------------------------------------------------------------------------
-- Helper: register a key idempotently. Existing rows are left untouched so
-- this migration is safe to re-run after operators have edited values.
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION _qtss_register_key(
    p_key             TEXT,
    p_category        TEXT,
    p_subcategory     TEXT,
    p_value_type      TEXT,
    p_default         JSONB,
    p_unit            TEXT,
    p_description     TEXT,
    p_ui_widget       TEXT,
    p_requires_restart BOOLEAN,
    p_sensitivity     TEXT,
    p_tags            TEXT[]
) RETURNS VOID AS $$
BEGIN
    INSERT INTO config_schema (
        key, category, subcategory, value_type, default_value,
        unit, description, ui_widget, requires_restart, sensitivity,
        introduced_in, tags
    ) VALUES (
        p_key, p_category, p_subcategory, p_value_type, p_default,
        p_unit, p_description, p_ui_widget, p_requires_restart, p_sensitivity,
        '0016', p_tags
    )
    ON CONFLICT (key) DO NOTHING;
END;
$$ LANGUAGE plpgsql;

-- ---------------------------------------------------------------------------
-- Risk limits — most stop-the-bleeding tunables. Defaults per §10B.
-- ---------------------------------------------------------------------------
SELECT _qtss_register_key('risk.max_drawdown_pct',           'risk', 'limits', 'float',
    '0.05'::jsonb, 'pct',
    'Account-level max drawdown before kill switch trips. 5% = halt all new entries.',
    'slider', false, 'high', ARRAY['risk','killswitch']);

SELECT _qtss_register_key('risk.max_daily_loss_pct',         'risk', 'limits', 'float',
    '0.02'::jsonb, 'pct',
    'Max realised+unrealised loss per UTC day before halt. 2% default.',
    'slider', false, 'high', ARRAY['risk']);

SELECT _qtss_register_key('risk.max_open_positions',         'risk', 'limits', 'int',
    '8'::jsonb, 'count',
    'Hard cap on concurrently open positions across all venues.',
    'number', false, 'high', ARRAY['risk']);

SELECT _qtss_register_key('risk.max_pos_per_instrument_pct', 'risk', 'limits', 'float',
    '0.10'::jsonb, 'pct',
    'Max equity allocation per instrument. 10% = no single name above 10% of NAV.',
    'slider', false, 'high', ARRAY['risk','sizing']);

SELECT _qtss_register_key('risk.max_leverage',               'risk', 'limits', 'float',
    '1.0'::jsonb, 'x',
    'Account-wide max leverage. 1.0 = spot only; >1 enables futures sizing.',
    'number', false, 'high', ARRAY['risk','futures']);

SELECT _qtss_register_key('risk.killswitch_dd_pct',          'risk', 'killswitch', 'float',
    '0.08'::jsonb, 'pct',
    'Drawdown that triggers a *full* trading halt (not just new-entry block).',
    'slider', false, 'high', ARRAY['killswitch']);

SELECT _qtss_register_key('risk.cooldown_after_loss_min',    'risk', 'sizing', 'int',
    '30'::jsonb, 'minutes',
    'Forced cooldown window after a losing trade closes before re-entry.',
    'number', false, 'normal', ARRAY['risk','sizing']);

SELECT _qtss_register_key('risk.position_sizing_method',     'risk', 'sizing', 'enum',
    '"atr_volatility"'::jsonb, NULL,
    'Default sizing method: fixed_pct | atr_volatility | kelly_fraction.',
    'select', false, 'normal', ARRAY['risk','sizing']);

-- ---------------------------------------------------------------------------
-- Execution — venue-agnostic order behaviour.
-- ---------------------------------------------------------------------------
SELECT _qtss_register_key('execution.default_order_type',    'execution', 'orders', 'enum',
    '"limit"'::jsonb, NULL,
    'Default order type when strategy does not specify: market | limit | stop_limit.',
    'select', false, 'normal', ARRAY['execution']);

SELECT _qtss_register_key('execution.slippage_bps',          'execution', 'orders', 'float',
    '5.0'::jsonb, 'bps',
    'Assumed slippage budget for market/aggressive orders.',
    'number', false, 'normal', ARRAY['execution','simulation']);

SELECT _qtss_register_key('execution.retry_on_reject',       'execution', 'orders', 'int',
    '2'::jsonb, 'count',
    'How many times to retry an order rejected for transient reasons.',
    'number', false, 'normal', ARRAY['execution']);

SELECT _qtss_register_key('execution.cancel_unfilled_after_s','execution', 'orders', 'int',
    '60'::jsonb, 'seconds',
    'Auto-cancel a working limit order if not filled within this window.',
    'number', false, 'normal', ARRAY['execution']);

-- ---------------------------------------------------------------------------
-- Market data ingestion.
-- ---------------------------------------------------------------------------
SELECT _qtss_register_key('market_data.timeframes',          'market_data', 'bars', 'array',
    '["1m","5m","15m","1h","4h","1d"]'::jsonb, NULL,
    'Active timeframes that detectors and the pivot engine consume.',
    'multiselect', true, 'normal', ARRAY['market_data']);

SELECT _qtss_register_key('market_data.ws_reconnect_backoff_ms','market_data','ingest','int',
    '1000'::jsonb, 'ms',
    'Initial backoff for venue WebSocket reconnects. Exponential up to 30s.',
    'number', false, 'normal', ARRAY['market_data','reliability']);

SELECT _qtss_register_key('market_data.bar_close_grace_ms',  'market_data', 'bars', 'int',
    '250'::jsonb, 'ms',
    'Grace period after a bar boundary before publishing bar.closed (lets late ticks settle).',
    'number', false, 'normal', ARRAY['market_data']);

-- ---------------------------------------------------------------------------
-- Pivots / zigzag — central pivot engine.
-- ---------------------------------------------------------------------------
SELECT _qtss_register_key('pivots.zigzag.atr_period',        'pivots', 'zigzag', 'int',
    '14'::jsonb, 'bars',
    'ATR lookback used by the zigzag swing-size threshold.',
    'number', false, 'normal', ARRAY['pivots']);

SELECT _qtss_register_key('pivots.zigzag.atr_mult_l0',       'pivots', 'zigzag', 'float',
    '1.5'::jsonb, 'x',
    'L0 (finest) zigzag threshold = atr_mult * ATR.',
    'number', false, 'normal', ARRAY['pivots']);

SELECT _qtss_register_key('pivots.zigzag.atr_mult_l1',       'pivots', 'zigzag', 'float',
    '3.0'::jsonb, 'x', 'L1 zigzag threshold multiplier.',
    'number', false, 'normal', ARRAY['pivots']);

SELECT _qtss_register_key('pivots.zigzag.atr_mult_l2',       'pivots', 'zigzag', 'float',
    '6.0'::jsonb, 'x', 'L2 zigzag threshold multiplier.',
    'number', false, 'normal', ARRAY['pivots']);

SELECT _qtss_register_key('pivots.zigzag.atr_mult_l3',       'pivots', 'zigzag', 'float',
    '12.0'::jsonb, 'x', 'L3 (coarsest) zigzag threshold multiplier.',
    'number', false, 'normal', ARRAY['pivots']);

-- ---------------------------------------------------------------------------
-- Regime detection.
-- ---------------------------------------------------------------------------
SELECT _qtss_register_key('regime.adx_period',               'regime', 'indicators', 'int',
    '14'::jsonb, 'bars', 'ADX lookback for trend strength.',
    'number', false, 'normal', ARRAY['regime']);

SELECT _qtss_register_key('regime.adx_trend_threshold',      'regime', 'indicators', 'float',
    '25.0'::jsonb, NULL, 'ADX value above which a trending regime is declared.',
    'number', false, 'normal', ARRAY['regime']);

SELECT _qtss_register_key('regime.bb_squeeze_threshold',     'regime', 'indicators', 'float',
    '0.05'::jsonb, 'pct',
    'Bollinger band width below which a squeeze (range) regime is declared.',
    'number', false, 'normal', ARRAY['regime']);

-- ---------------------------------------------------------------------------
-- Pattern detection — confidence floors per family.
-- ---------------------------------------------------------------------------
SELECT _qtss_register_key('detection.elliott.min_confidence',  'detection','elliott','float',
    '0.55'::jsonb, NULL,
    'Validator confidence floor for Elliott wave detections to publish pattern.validated.',
    'slider', false, 'normal', ARRAY['detection','elliott']);

SELECT _qtss_register_key('detection.harmonic.min_confidence', 'detection','harmonic','float',
    '0.60'::jsonb, NULL, 'Confidence floor for harmonic patterns (XABCD).',
    'slider', false, 'normal', ARRAY['detection','harmonic']);

SELECT _qtss_register_key('detection.classical.min_confidence','detection','classical','float',
    '0.55'::jsonb, NULL, 'Confidence floor for classical chart patterns (H&S, triangles, ...).',
    'slider', false, 'normal', ARRAY['detection','classical']);

SELECT _qtss_register_key('detection.wyckoff.min_confidence',  'detection','wyckoff','float',
    '0.60'::jsonb, NULL, 'Confidence floor for Wyckoff phase detections.',
    'slider', false, 'normal', ARRAY['detection','wyckoff']);

-- ---------------------------------------------------------------------------
-- Scheduler — periodic data pulls (Nansen, on-chain feeds, etc.).
-- ---------------------------------------------------------------------------
SELECT _qtss_register_key('scheduler.tick_interval_s',       'scheduler', 'core', 'int',
    '5'::jsonb, 'seconds', 'Scheduler poll tick — how often it checks for due jobs.',
    'number', true, 'normal', ARRAY['scheduler']);

SELECT _qtss_register_key('scheduler.max_concurrent_jobs',   'scheduler', 'core', 'int',
    '4'::jsonb, 'count', 'Upper bound on concurrent scheduled job runs.',
    'number', true, 'normal', ARRAY['scheduler']);

-- ---------------------------------------------------------------------------
-- Notifications & telegram (operator alerts).
-- ---------------------------------------------------------------------------
SELECT _qtss_register_key('notify.telegram.enabled',         'notify', 'telegram', 'bool',
    'false'::jsonb, NULL, 'Master switch for Telegram alert delivery.',
    'switch', false, 'normal', ARRAY['notify']);

SELECT _qtss_register_key('notify.telegram.bot_token_secret','notify','telegram','string',
    '"telegram.bot_token"'::jsonb, NULL,
    'Name of the secret in secrets_vault holding the Telegram bot token.',
    'text', false, 'high', ARRAY['notify','secrets']);

-- ---------------------------------------------------------------------------
-- Run mode — global default. Per-worker overrides go in code via RunMode.
-- ---------------------------------------------------------------------------
SELECT _qtss_register_key('runtime.default_mode',            'runtime', 'mode', 'enum',
    '"dry"'::jsonb, NULL,
    'Default run mode when a worker does not specify one: live | dry | backtest. Default DRY for safety.',
    'select', true, 'high', ARRAY['runtime']);

-- ---------------------------------------------------------------------------
-- Asset-class overrides — example: tighter risk for futures.
-- These insert into config_value, not config_schema. They demonstrate the
-- scope-override mechanism so the GUI shows real precedence on day one.
-- ---------------------------------------------------------------------------
INSERT INTO config_value (key, scope_id, value)
SELECT 'risk.max_leverage', cs.id, '3.0'::jsonb
FROM config_scope cs
WHERE cs.scope_type = 'asset_class' AND cs.scope_key = 'crypto_futures'
ON CONFLICT (key, scope_id) DO NOTHING;

INSERT INTO config_value (key, scope_id, value)
SELECT 'risk.max_open_positions', cs.id, '4'::jsonb
FROM config_scope cs
WHERE cs.scope_type = 'asset_class' AND cs.scope_key = 'crypto_futures'
ON CONFLICT (key, scope_id) DO NOTHING;

DROP FUNCTION _qtss_register_key(
    TEXT, TEXT, TEXT, TEXT, JSONB, TEXT, TEXT, TEXT, BOOLEAN, TEXT, TEXT[]
);

COMMIT;
