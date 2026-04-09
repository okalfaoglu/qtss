-- 0032_qtss_v2_setups.sql
--
-- Faz 8.0 — Setup Engine foundation.
--
-- Storage for the setup lifecycle (armed/active/closed), event
-- outbox for Telegram/tracing consumers, rejection audit for the
-- allocator, and a correlation-group lookup table used to enforce
-- the per-group cap.
--
-- All engine knobs (risk caps, ratchet intervals, thresholds,
-- per-venue enable flags, notify toggles) are registered in
-- `system_config` at the bottom of the file so they can be tuned
-- from the GUI without a redeploy (CLAUDE.md #2).

-- ───────────────────────── 1. qtss_v2_setups ───────────────────────────

CREATE TABLE IF NOT EXISTS qtss_v2_setups (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    venue_class TEXT NOT NULL CHECK (venue_class IN ('crypto','bist','us_equities','commodities','fx')),
    exchange TEXT NOT NULL,
    symbol TEXT NOT NULL,
    timeframe TEXT NOT NULL,
    profile TEXT NOT NULL CHECK (profile IN ('t','q','d')),
    alt_type TEXT CHECK (alt_type IN ('reaction_low','trend_low','reversal_high','selling_high')),
    state TEXT NOT NULL CHECK (state IN ('flat','armed','active','closed')),
    direction TEXT NOT NULL CHECK (direction IN ('long','short','neutral')),
    confluence_id UUID REFERENCES qtss_v2_confluence(id),
    entry_price REAL,
    entry_sl REAL,
    koruma REAL,
    target_ref REAL,
    risk_pct REAL,
    close_reason TEXT,
    close_price REAL,
    closed_at TIMESTAMPTZ,
    raw_meta JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_v2_setups_open
    ON qtss_v2_setups (venue_class, profile, state)
    WHERE state IN ('armed','active');
CREATE INDEX IF NOT EXISTS idx_v2_setups_symbol
    ON qtss_v2_setups (exchange, symbol, timeframe, created_at DESC);

-- ───────────────────────── 2. qtss_v2_setup_events ─────────────────────

CREATE TABLE IF NOT EXISTS qtss_v2_setup_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    setup_id UUID NOT NULL REFERENCES qtss_v2_setups(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    event_type TEXT NOT NULL CHECK (event_type IN ('opened','updated','closed','rejected')),
    payload JSONB NOT NULL,
    delivery_state TEXT NOT NULL DEFAULT 'pending'
        CHECK (delivery_state IN ('pending','delivered','failed','skipped')),
    delivered_at TIMESTAMPTZ,
    retries INTEGER NOT NULL DEFAULT 0,
    UNIQUE (setup_id, event_type, created_at)
);

CREATE INDEX IF NOT EXISTS idx_setup_events_pending
    ON qtss_v2_setup_events (delivery_state, created_at)
    WHERE delivery_state = 'pending';

-- ───────────────────────── 3. qtss_v2_setup_rejections ─────────────────

CREATE TABLE IF NOT EXISTS qtss_v2_setup_rejections (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    venue_class TEXT NOT NULL,
    exchange TEXT NOT NULL,
    symbol TEXT NOT NULL,
    timeframe TEXT NOT NULL,
    profile TEXT NOT NULL,
    direction TEXT NOT NULL,
    reject_reason TEXT NOT NULL
        CHECK (reject_reason IN ('total_risk_cap','max_concurrent','correlation_cap')),
    confluence_id UUID REFERENCES qtss_v2_confluence(id),
    raw_meta JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_setup_rejections_recent
    ON qtss_v2_setup_rejections (created_at DESC);

-- ───────────────────────── 4. qtss_v2_correlation_groups ───────────────

CREATE TABLE IF NOT EXISTS qtss_v2_correlation_groups (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    venue_class TEXT NOT NULL
        CHECK (venue_class IN ('crypto','bist','us_equities','commodities','fx')),
    group_key TEXT NOT NULL,
    symbol TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 1.0,
    UNIQUE (venue_class, group_key, symbol)
);

CREATE INDEX IF NOT EXISTS idx_corr_groups_lookup
    ON qtss_v2_correlation_groups (venue_class, symbol);

-- ───────────────────── correlation group seeds ─────────────────────────
-- Crypto
INSERT INTO qtss_v2_correlation_groups (venue_class, group_key, symbol) VALUES
    ('crypto','btc_family','BTC'),
    ('crypto','btc_family','WBTC'),
    ('crypto','eth_family','ETH'),
    ('crypto','eth_family','WETH'),
    ('crypto','eth_family','STETH'),
    ('crypto','large_cap_alt','SOL'),
    ('crypto','large_cap_alt','BNB'),
    ('crypto','large_cap_alt','XRP'),
    ('crypto','large_cap_alt','ADA'),
    ('crypto','defi','UNI'),
    ('crypto','defi','AAVE'),
    ('crypto','defi','LINK'),
    ('crypto','defi','MKR'),
    ('crypto','meme','DOGE'),
    ('crypto','meme','SHIB'),
    ('crypto','meme','PEPE')
ON CONFLICT (venue_class, group_key, symbol) DO NOTHING;

-- BIST
INSERT INTO qtss_v2_correlation_groups (venue_class, group_key, symbol) VALUES
    ('bist','banking','AKBNK'),
    ('bist','banking','GARAN'),
    ('bist','banking','ISCTR'),
    ('bist','banking','YKBNK'),
    ('bist','banking','HALKB'),
    ('bist','banking','VAKBN'),
    ('bist','holding','KCHOL'),
    ('bist','holding','SAHOL'),
    ('bist','holding','DOHOL'),
    ('bist','holding','ENKAI'),
    ('bist','defense','ASELS'),
    ('bist','defense','OTKAR'),
    ('bist','defense','KONTR'),
    ('bist','energy','TUPRS'),
    ('bist','energy','AYGAZ'),
    ('bist','energy','AKSEN'),
    ('bist','iron_steel','EREGL'),
    ('bist','iron_steel','KRDMD'),
    ('bist','iron_steel','ISDMR')
ON CONFLICT (venue_class, group_key, symbol) DO NOTHING;

-- ───────────────────────── config keys ─────────────────────────────────
-- Master switch + loop cadence
SELECT _qtss_register_key('setup.engine.enabled','setup','engine','bool',
    'false'::jsonb, NULL,
    'Master switch for the Faz 8.0 Setup Engine loop. Default OFF — ops turns on per-environment.',
    'toggle', true, 'normal', ARRAY['setup','engine']);
SELECT _qtss_register_key('setup.engine.tick_interval_s','setup','engine','int',
    '30'::jsonb, NULL,
    'How often the setup engine loop evaluates open setups and new candidates (seconds).',
    'number', true, 'normal', ARRAY['setup','engine']);

-- Profile T (short-term)
SELECT _qtss_register_key('setup.guard.t.entry_sl_atr_mult','setup','guard','float',
    '1.0'::jsonb, NULL, 'T profile: initial stop distance in ATR multiples.',
    'number', true, 'normal', ARRAY['setup','guard','t']);
SELECT _qtss_register_key('setup.guard.t.target_ref_r','setup','guard','float',
    '1.5'::jsonb, NULL, 'T profile: target distance from entry in R multiples.',
    'number', true, 'normal', ARRAY['setup','guard','t']);
SELECT _qtss_register_key('setup.guard.t.risk_pct','setup','guard','float',
    '0.25'::jsonb, NULL, 'T profile: per-setup risk as percent of equity.',
    'number', true, 'normal', ARRAY['setup','guard','t']);
SELECT _qtss_register_key('setup.guard.t.max_concurrent','setup','guard','int',
    '4'::jsonb, NULL, 'T profile: maximum concurrent open setups.',
    'number', true, 'normal', ARRAY['setup','guard','t']);
SELECT _qtss_register_key('setup.guard.t.ratchet_interval_s','setup','guard','int',
    '900'::jsonb, NULL, 'T profile: minimum seconds between ratchet tightenings (15m default).',
    'number', true, 'normal', ARRAY['setup','guard','t']);

-- Profile Q (short-mid)
SELECT _qtss_register_key('setup.guard.q.entry_sl_atr_mult','setup','guard','float',
    '1.5'::jsonb, NULL, 'Q profile: initial stop distance in ATR multiples.',
    'number', true, 'normal', ARRAY['setup','guard','q']);
SELECT _qtss_register_key('setup.guard.q.target_ref_r','setup','guard','float',
    '2.5'::jsonb, NULL, 'Q profile: target distance from entry in R multiples.',
    'number', true, 'normal', ARRAY['setup','guard','q']);
SELECT _qtss_register_key('setup.guard.q.risk_pct','setup','guard','float',
    '0.50'::jsonb, NULL, 'Q profile: per-setup risk as percent of equity.',
    'number', true, 'normal', ARRAY['setup','guard','q']);
SELECT _qtss_register_key('setup.guard.q.max_concurrent','setup','guard','int',
    '3'::jsonb, NULL, 'Q profile: maximum concurrent open setups.',
    'number', true, 'normal', ARRAY['setup','guard','q']);
SELECT _qtss_register_key('setup.guard.q.ratchet_interval_s','setup','guard','int',
    '3600'::jsonb, NULL, 'Q profile: minimum seconds between ratchet tightenings (1h default).',
    'number', true, 'normal', ARRAY['setup','guard','q']);

-- Profile D (mid/long)
SELECT _qtss_register_key('setup.guard.d.entry_sl_atr_mult','setup','guard','float',
    '2.5'::jsonb, NULL, 'D profile: initial stop distance in ATR multiples.',
    'number', true, 'normal', ARRAY['setup','guard','d']);
SELECT _qtss_register_key('setup.guard.d.target_ref_r','setup','guard','float',
    '4.0'::jsonb, NULL, 'D profile: target distance from entry in R multiples.',
    'number', true, 'normal', ARRAY['setup','guard','d']);
SELECT _qtss_register_key('setup.guard.d.risk_pct','setup','guard','float',
    '1.00'::jsonb, NULL, 'D profile: per-setup risk as percent of equity.',
    'number', true, 'normal', ARRAY['setup','guard','d']);
SELECT _qtss_register_key('setup.guard.d.max_concurrent','setup','guard','int',
    '2'::jsonb, NULL, 'D profile: maximum concurrent open setups.',
    'number', true, 'normal', ARRAY['setup','guard','d']);
SELECT _qtss_register_key('setup.guard.d.ratchet_interval_s','setup','guard','int',
    '86400'::jsonb, NULL, 'D profile: minimum seconds between ratchet tightenings (1d default).',
    'number', true, 'normal', ARRAY['setup','guard','d']);

-- Allocator caps
SELECT _qtss_register_key('setup.risk.crypto.max_total_open_risk_pct','setup','risk','float',
    '6.0'::jsonb, NULL, 'Crypto: hard cap on the sum of risk_pct across all open setups.',
    'number', true, 'normal', ARRAY['setup','risk','crypto']);
SELECT _qtss_register_key('setup.risk.bist.max_total_open_risk_pct','setup','risk','float',
    '6.0'::jsonb, NULL, 'BIST: hard cap on the sum of risk_pct across all open setups.',
    'number', true, 'normal', ARRAY['setup','risk','bist']);
SELECT _qtss_register_key('setup.risk.correlation.enabled','setup','risk','bool',
    'true'::jsonb, NULL, 'Enable correlation-group cap in the allocator.',
    'toggle', true, 'normal', ARRAY['setup','risk','correlation']);
SELECT _qtss_register_key('setup.risk.correlation.max_per_group','setup','risk','int',
    '2'::jsonb, NULL, 'Allocator: maximum concurrent open setups sharing a correlation group.',
    'number', true, 'normal', ARRAY['setup','risk','correlation']);
SELECT _qtss_register_key('setup.risk.correlation.same_direction_only','setup','risk','bool',
    'true'::jsonb, NULL, 'Allocator: only count setups in the same direction toward the correlation cap.',
    'toggle', true, 'normal', ARRAY['setup','risk','correlation']);

-- Reverse-signal evaluator
SELECT _qtss_register_key('setup.reverse.enabled','setup','reverse','bool',
    'true'::jsonb, NULL, 'Enable the reverse-signal early close evaluator.',
    'toggle', true, 'normal', ARRAY['setup','reverse']);
SELECT _qtss_register_key('setup.reverse.t.guven_threshold','setup','reverse','float',
    '0.65'::jsonb, NULL, 'T profile: guven threshold above which a reverse signal force-closes an active setup.',
    'number', true, 'normal', ARRAY['setup','reverse','t']);
SELECT _qtss_register_key('setup.reverse.q.guven_threshold','setup','reverse','float',
    '0.55'::jsonb, NULL, 'Q profile: guven threshold above which a reverse signal force-closes an active setup.',
    'number', true, 'normal', ARRAY['setup','reverse','q']);
SELECT _qtss_register_key('setup.reverse.d.guven_threshold','setup','reverse','float',
    '0.70'::jsonb, NULL, 'D profile: guven threshold above which a reverse signal force-closes an active setup.',
    'number', true, 'normal', ARRAY['setup','reverse','d']);

-- Venue toggles
SELECT _qtss_register_key('setup.venue.crypto.enabled','setup','venue','bool',
    'true'::jsonb, NULL, 'Enable the Setup Engine for the crypto venue class.',
    'toggle', true, 'normal', ARRAY['setup','venue','crypto']);
SELECT _qtss_register_key('setup.venue.bist.enabled','setup','venue','bool',
    'true'::jsonb, NULL, 'Enable the Setup Engine for the BIST venue class.',
    'toggle', true, 'normal', ARRAY['setup','venue','bist']);
SELECT _qtss_register_key('setup.venue.us_equities.enabled','setup','venue','bool',
    'false'::jsonb, NULL, 'Enable the Setup Engine for US equities (schema-only in Faz 8.0).',
    'toggle', true, 'normal', ARRAY['setup','venue','us_equities']);
SELECT _qtss_register_key('setup.venue.commodities.enabled','setup','venue','bool',
    'false'::jsonb, NULL, 'Enable the Setup Engine for commodities (schema-only in Faz 8.0).',
    'toggle', true, 'normal', ARRAY['setup','venue','commodities']);
SELECT _qtss_register_key('setup.venue.fx.enabled','setup','venue','bool',
    'false'::jsonb, NULL, 'Enable the Setup Engine for FX (schema-only in Faz 8.0).',
    'toggle', true, 'normal', ARRAY['setup','venue','fx']);

-- Notification channels
SELECT _qtss_register_key('setup.notify.dblog.enabled','setup','notify','bool',
    'true'::jsonb, NULL, 'Write setup lifecycle events to qtss_v2_setup_events.',
    'toggle', true, 'normal', ARRAY['setup','notify']);
SELECT _qtss_register_key('setup.notify.tracing.enabled','setup','notify','bool',
    'true'::jsonb, NULL, 'Emit tracing spans for each setup lifecycle transition.',
    'toggle', true, 'normal', ARRAY['setup','notify']);
SELECT _qtss_register_key('setup.notify.telegram.enabled','setup','notify','bool',
    'true'::jsonb, NULL, 'Forward setup lifecycle events to Telegram.',
    'toggle', true, 'normal', ARRAY['setup','notify','telegram']);
SELECT _qtss_register_key('setup.notify.telegram.attach_chart','setup','notify','bool',
    'true'::jsonb, NULL, 'Attach a rendered chart image to Telegram setup notifications.',
    'toggle', true, 'normal', ARRAY['setup','notify','telegram']);
SELECT _qtss_register_key('setup.notify.telegram.chart_lookback_bars','setup','notify','int',
    '200'::jsonb, NULL, 'How many bars the Telegram chart attachment should render.',
    'number', true, 'normal', ARRAY['setup','notify','telegram']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('setup','engine.enabled',                         'false'::jsonb, 'Master switch for the Setup Engine loop.'),
    ('setup','engine.tick_interval_s',                 '30'::jsonb,    'Setup engine loop cadence (s).'),
    ('setup','guard.t.entry_sl_atr_mult',              '1.0'::jsonb,   'T: initial SL in ATR multiples.'),
    ('setup','guard.t.target_ref_r',                   '1.5'::jsonb,   'T: target in R multiples.'),
    ('setup','guard.t.risk_pct',                       '0.25'::jsonb,  'T: risk percent per setup.'),
    ('setup','guard.t.max_concurrent',                 '4'::jsonb,     'T: max concurrent open setups.'),
    ('setup','guard.t.ratchet_interval_s',             '900'::jsonb,   'T: min seconds between ratchet tightenings.'),
    ('setup','guard.q.entry_sl_atr_mult',              '1.5'::jsonb,   'Q: initial SL in ATR multiples.'),
    ('setup','guard.q.target_ref_r',                   '2.5'::jsonb,   'Q: target in R multiples.'),
    ('setup','guard.q.risk_pct',                       '0.50'::jsonb,  'Q: risk percent per setup.'),
    ('setup','guard.q.max_concurrent',                 '3'::jsonb,     'Q: max concurrent open setups.'),
    ('setup','guard.q.ratchet_interval_s',             '3600'::jsonb,  'Q: min seconds between ratchet tightenings.'),
    ('setup','guard.d.entry_sl_atr_mult',              '2.5'::jsonb,   'D: initial SL in ATR multiples.'),
    ('setup','guard.d.target_ref_r',                   '4.0'::jsonb,   'D: target in R multiples.'),
    ('setup','guard.d.risk_pct',                       '1.00'::jsonb,  'D: risk percent per setup.'),
    ('setup','guard.d.max_concurrent',                 '2'::jsonb,     'D: max concurrent open setups.'),
    ('setup','guard.d.ratchet_interval_s',             '86400'::jsonb, 'D: min seconds between ratchet tightenings.'),
    ('setup','risk.crypto.max_total_open_risk_pct',    '6.0'::jsonb,   'Crypto: total open risk cap.'),
    ('setup','risk.bist.max_total_open_risk_pct',      '6.0'::jsonb,   'BIST: total open risk cap.'),
    ('setup','risk.correlation.enabled',               'true'::jsonb,  'Enable correlation cap.'),
    ('setup','risk.correlation.max_per_group',         '2'::jsonb,     'Max open setups per correlation group.'),
    ('setup','risk.correlation.same_direction_only',   'true'::jsonb,  'Only count same-direction setups.'),
    ('setup','reverse.enabled',                        'true'::jsonb,  'Enable reverse-signal early close.'),
    ('setup','reverse.t.guven_threshold',              '0.65'::jsonb,  'T: reverse close guven threshold.'),
    ('setup','reverse.q.guven_threshold',              '0.55'::jsonb,  'Q: reverse close guven threshold.'),
    ('setup','reverse.d.guven_threshold',              '0.70'::jsonb,  'D: reverse close guven threshold.'),
    ('setup','venue.crypto.enabled',                   'true'::jsonb,  'Crypto venue enabled.'),
    ('setup','venue.bist.enabled',                     'true'::jsonb,  'BIST venue enabled.'),
    ('setup','venue.us_equities.enabled',              'false'::jsonb, 'US equities venue enabled.'),
    ('setup','venue.commodities.enabled',              'false'::jsonb, 'Commodities venue enabled.'),
    ('setup','venue.fx.enabled',                       'false'::jsonb, 'FX venue enabled.'),
    ('setup','notify.dblog.enabled',                   'true'::jsonb,  'Write setup events to DB outbox.'),
    ('setup','notify.tracing.enabled',                 'true'::jsonb,  'Emit tracing spans on setup transitions.'),
    ('setup','notify.telegram.enabled',                'true'::jsonb,  'Forward setup events to Telegram.'),
    ('setup','notify.telegram.attach_chart',           'true'::jsonb,  'Attach chart image to Telegram setup messages.'),
    ('setup','notify.telegram.chart_lookback_bars',    '200'::jsonb,   'Lookback bars for Telegram chart render.')
ON CONFLICT (module, config_key) DO NOTHING;
