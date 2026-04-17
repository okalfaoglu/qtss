-- Faz 9.7.0 — Config keys for Position Health, Poz Koruma,
-- Smart Target AI, Score Tiers, Lifecycle notifications.
--
-- CLAUDE.md #2 — every threshold here is DB-driven, editable from
-- the Config Editor GUI, audited. No hardcoded magic numbers in Rust.

-- ============================================================
-- Public-facing score tiers (0..1 -> N/10 + label)
-- ============================================================
SELECT _qtss_register_key(
    'public_card.tier.orta_min', 'notify', 'public_card',
    'float', '0.40'::jsonb, '',
    'AI score >= this maps to ORTA (5/10). Below -> ZAYIF.',
    'number', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'public_card.tier.guclu_min', 'notify', 'public_card',
    'float', '0.55'::jsonb, '',
    'AI score >= this maps to GUCLU (7/10).',
    'number', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'public_card.tier.cok_guclu_min', 'notify', 'public_card',
    'float', '0.70'::jsonb, '',
    'AI score >= this maps to COK_GUCLU (9/10).',
    'number', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'public_card.tier.mukemmel_min', 'notify', 'public_card',
    'float', '0.85'::jsonb, '',
    'AI score >= this maps to MUKEMMEL (10/10).',
    'number', false, 'normal', ARRAY['notify','faz97']
);

-- ============================================================
-- Position Health Score — component weights + band thresholds
-- ============================================================
SELECT _qtss_register_key(
    'health.weight.momentum', 'notify', 'health',
    'float', '0.25'::jsonb, '',
    'Weight of momentum component in Position Health Score.',
    'number', false, 'normal', ARRAY['health','faz97']
);
SELECT _qtss_register_key(
    'health.weight.structural', 'notify', 'health',
    'float', '0.25'::jsonb, '',
    'Weight of structural (pattern integrity) component.',
    'number', false, 'normal', ARRAY['health','faz97']
);
SELECT _qtss_register_key(
    'health.weight.orderbook', 'notify', 'health',
    'float', '0.15'::jsonb, '',
    'Weight of orderbook component.',
    'number', false, 'normal', ARRAY['health','faz97']
);
SELECT _qtss_register_key(
    'health.weight.regime', 'notify', 'health',
    'float', '0.15'::jsonb, '',
    'Weight of regime-match component.',
    'number', false, 'normal', ARRAY['health','faz97']
);
SELECT _qtss_register_key(
    'health.weight.correlation', 'notify', 'health',
    'float', '0.10'::jsonb, '',
    'Weight of correlation component (BTC.D / dominant asset).',
    'number', false, 'normal', ARRAY['health','faz97']
);
SELECT _qtss_register_key(
    'health.weight.ai_rescore', 'notify', 'health',
    'float', '0.10'::jsonb, '',
    'Weight of periodic AI re-inference component.',
    'number', false, 'normal', ARRAY['health','faz97']
);
-- Band thresholds (health score out of 100).
SELECT _qtss_register_key(
    'health.band.healthy_min', 'notify', 'health',
    'float', '70.0'::jsonb, '',
    'Score >= this is HEALTHY band.',
    'number', false, 'normal', ARRAY['health','faz97']
);
SELECT _qtss_register_key(
    'health.band.warn_min', 'notify', 'health',
    'float', '50.0'::jsonb, '',
    'Score >= this (and < healthy_min) is WARN band.',
    'number', false, 'normal', ARRAY['health','faz97']
);
SELECT _qtss_register_key(
    'health.band.danger_min', 'notify', 'health',
    'float', '30.0'::jsonb, '',
    'Score >= this (and < warn_min) is DANGER band. Below -> CRITICAL.',
    'number', false, 'normal', ARRAY['health','faz97']
);
-- AI trigger thresholds.
SELECT _qtss_register_key(
    'health.smart_target.rule_below', 'notify', 'health',
    'float', '50.0'::jsonb, '',
    'Health score < this invokes rule-based Smart Target evaluator.',
    'number', false, 'normal', ARRAY['health','faz97']
);
SELECT _qtss_register_key(
    'health.smart_target.llm_below', 'notify', 'health',
    'float', '30.0'::jsonb, '',
    'Health score < this invokes LLM Smart Target judge.',
    'number', false, 'normal', ARRAY['health','faz97']
);
-- AI rescore cadence.
SELECT _qtss_register_key(
    'health.ai_rescore.interval_secs', 'notify', 'health',
    'int', '300'::jsonb, '',
    'Seconds between periodic AI re-inference for active positions.',
    'number', false, 'normal', ARRAY['health','faz97']
);

-- ============================================================
-- Poz Koruma (daily profit ratchet, variant A1)
-- ============================================================
SELECT _qtss_register_key(
    'poz_koruma.enabled', 'notify', 'poz_koruma',
    'bool', 'true'::jsonb, '',
    'Enable daily profit ratchet. SL trails up by cumulative daily gain.',
    'bool', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'poz_koruma.eod_hour_utc', 'notify', 'poz_koruma',
    'int', '0'::jsonb, '',
    'UTC hour considered end-of-day for ratchet computation (0..23).',
    'number', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'poz_koruma.min_gain_pct', 'notify', 'poz_koruma',
    'float', '0.5'::jsonb, '',
    'Minimum daily gain %% required to trigger a ratchet step (avoids noise).',
    'number', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'poz_koruma.buffer_pct', 'notify', 'poz_koruma',
    'float', '0.05'::jsonb, '',
    'Safety buffer subtracted from ratcheted SL to avoid wick-outs.',
    'number', false, 'normal', ARRAY['notify','faz97']
);

-- ============================================================
-- Price tick buffer + watcher
-- ============================================================
SELECT _qtss_register_key(
    'price_watcher.enabled', 'notify', 'price_watcher',
    'bool', 'false'::jsonb, '',
    'Enable tick-level price watcher (bookTicker stream + setup evaluation).',
    'bool', true, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'price_watcher.stream', 'notify', 'price_watcher',
    'string', '"bookTicker"'::jsonb, '',
    'Binance WS stream type: bookTicker (every tick) | miniTicker | aggTrade.',
    'text', true, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'price_watcher.max_active', 'notify', 'price_watcher',
    'int', '500'::jsonb, '',
    'Max simultaneous active setups tracked by watcher.',
    'number', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'price_watcher.health_persist_min_band_delta', 'notify', 'price_watcher',
    'int', '1'::jsonb, '',
    'Persist health snapshot only when band index changes by >= this (1=any crossing).',
    'number', false, 'normal', ARRAY['notify','faz97']
);

-- ============================================================
-- Lifecycle notification global defaults
-- (user-level override lives in notify_delivery_prefs)
-- ============================================================
SELECT _qtss_register_key(
    'notify.lifecycle.entry_touched.enabled', 'notify', 'lifecycle',
    'bool', 'true'::jsonb, '', 'Broadcast entry_touched events.',
    'bool', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'notify.lifecycle.tp_partial.enabled', 'notify', 'lifecycle',
    'bool', 'true'::jsonb, '', 'Broadcast partial TP events.',
    'bool', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'notify.lifecycle.tp_final.enabled', 'notify', 'lifecycle',
    'bool', 'true'::jsonb, '', 'Broadcast final TP (success close) events.',
    'bool', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'notify.lifecycle.sl_hit.enabled', 'notify', 'lifecycle',
    'bool', 'true'::jsonb, '', 'Broadcast SL hit (loss close) events.',
    'bool', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'notify.lifecycle.sl_ratcheted.enabled', 'notify', 'lifecycle',
    'bool', 'false'::jsonb, '', 'Broadcast Poz Koruma SL ratchet events (noisy, default off).',
    'bool', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'notify.lifecycle.invalidated.enabled', 'notify', 'lifecycle',
    'bool', 'true'::jsonb, '', 'Broadcast structural invalidation events.',
    'bool', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'notify.lifecycle.health_warn.enabled', 'notify', 'lifecycle',
    'bool', 'false'::jsonb, '', 'Broadcast health warn-band crossings (default off).',
    'bool', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'notify.lifecycle.health_danger.enabled', 'notify', 'lifecycle',
    'bool', 'true'::jsonb, '', 'Broadcast health danger-band crossings.',
    'bool', false, 'normal', ARRAY['notify','faz97']
);

-- Minimum score tier required for X (Twitter) publication.
SELECT _qtss_register_key(
    'x.publish.min_tier', 'notify', 'x',
    'int', '7'::jsonb, '',
    'Minimum 0-10 tier for a setup to be broadcast on X (avoid spam).',
    'number', false, 'normal', ARRAY['notify','faz97','x']
);
SELECT _qtss_register_key(
    'x.publish.daily_cap', 'notify', 'x',
    'int', '20'::jsonb, '',
    'Max number of X posts per UTC day.',
    'number', false, 'normal', ARRAY['notify','faz97','x']
);
SELECT _qtss_register_key(
    'x.publish.enabled', 'notify', 'x',
    'bool', 'false'::jsonb, '',
    'Master switch for X (Twitter) publication loop.',
    'bool', true, 'normal', ARRAY['notify','faz97','x']
);

-- ============================================================
-- Digest (per-user, tz-aware)
-- ============================================================
SELECT _qtss_register_key(
    'digest.daily.enabled', 'notify', 'digest',
    'bool', 'true'::jsonb, '', 'Enable per-user daily digest loop.',
    'bool', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'digest.daily.local_hour', 'notify', 'digest',
    'int', '0'::jsonb, '',
    'Local hour (0-23 in user tz) at which daily digest is delivered.',
    'number', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'digest.weekly.enabled', 'notify', 'digest',
    'bool', 'true'::jsonb, '', 'Enable per-user weekly digest (Sunday local).',
    'bool', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'digest.scheduler.tick_secs', 'notify', 'digest',
    'int', '300'::jsonb, '',
    'Seconds between digest scheduler ticks (checks each user tz).',
    'number', true, 'normal', ARRAY['notify','faz97']
);
