-- 0110_commission_gate_unified.sql
--
-- Faz 8 step 1 — unify the commission gate across the D/T/Q and Wyckoff
-- setup loops. Both now read from the same `resolve_commission_bps`
-- helper which walks this precedence ladder:
--
--   1. `commission.{venue_class}.taker_bps`   (venue-specific override)
--   2. `commission.taker_bps`                  (global, from migration 0047)
--   3. caller fallback                         (last-resort default)
--
-- Values taken from Binance's published taker schedule at tier 0:
--   * Spot     — 0.10%  = 10 bps
--   * Futures  — 0.05%  = 5  bps
--
-- Operators with VIP tier / BNB-discount seats can edit these rows
-- through the GUI without a deploy (CLAUDE.md #2).

-- Register the new venue-specific keys so they surface in the config
-- editor's schema view.
SELECT _qtss_register_key(
    'commission.binance_futures.taker_bps','setup','setup','float',
    '5.0'::jsonb, 'bps',
    'Binance futures taker commission (bps). Tier 0 default; VIP tiers can lower.',
    'number', true, 'normal', ARRAY['commission','binance','futures']);

SELECT _qtss_register_key(
    'commission.binance_futures.maker_bps','setup','setup','float',
    '2.0'::jsonb, 'bps',
    'Binance futures maker commission (bps). Tier 0 default.',
    'number', true, 'normal', ARRAY['commission','binance','futures']);

SELECT _qtss_register_key(
    'commission.binance_spot.taker_bps','setup','setup','float',
    '10.0'::jsonb, 'bps',
    'Binance spot taker commission (bps). Tier 0 default; VIP/BNB-discount tiers can lower.',
    'number', true, 'normal', ARRAY['commission','binance','spot']);

SELECT _qtss_register_key(
    'commission.binance_spot.maker_bps','setup','setup','float',
    '10.0'::jsonb, 'bps',
    'Binance spot maker commission (bps). Tier 0 default.',
    'number', true, 'normal', ARRAY['commission','binance','spot']);

-- Re-register the global key from 0047 with richer schema metadata so
-- the editor groups it under the same umbrella.
SELECT _qtss_register_key(
    'commission.taker_bps','setup','setup','float',
    '5.0'::jsonb, 'bps',
    'Global fallback taker commission (bps) when no venue-specific override is set.',
    'number', true, 'normal', ARRAY['commission','global']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('setup','commission.binance_futures.taker_bps','5.0'::jsonb,'Binance futures taker — tier 0.'),
    ('setup','commission.binance_futures.maker_bps','2.0'::jsonb,'Binance futures maker — tier 0.'),
    ('setup','commission.binance_spot.taker_bps','10.0'::jsonb,'Binance spot taker — tier 0.'),
    ('setup','commission.binance_spot.maker_bps','10.0'::jsonb,'Binance spot maker — tier 0.')
ON CONFLICT (module, config_key) DO NOTHING;
