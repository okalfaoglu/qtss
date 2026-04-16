-- 0104_wyckoff_sos_sow_gate.sql
--
-- P7.2 — SOS / SOW confirmation gate for Wyckoff setup builder.
-- Villahermosa *Wyckoff 2.0* §7.4.2: Phase-D continuation setups
-- (LPS / BUEC / LPSY / IceRetest) must be preceded by a fresh
-- Sign-of-Strength (long) or Sign-of-Weakness (short) proving that
-- the Composite Operator is driving price in the setup direction.
-- Phase-C climax setups (Spring / UT / UTAD) are exempt — they ARE
-- the initial trigger.

SELECT _qtss_register_key(
    'wyckoff.setup.require_sos_sow_trigger','setup','detection','bool',
    'true'::jsonb, 'flag',
    'Require SOS/SOW confirmation before Phase-D Wyckoff setups trigger.',
    'boolean', true, 'normal', ARRAY['wyckoff','setup','sos','sow']);

SELECT _qtss_register_key(
    'wyckoff.setup.sos_sow_max_bars_ago','setup','detection','int',
    '50'::jsonb, 'bars',
    'Max bars between SOS/SOW and the setup trigger for the confirmation to count. Default 50.',
    'number', true, 'normal', ARRAY['wyckoff','setup','sos','sow']);

SELECT _qtss_register_key(
    'wyckoff.setup.sos_sow_required_for','setup','detection','string',
    '"lps,buec,lpsy,ice_retest"'::jsonb, 'csv',
    'Comma-separated setup types that require SOS/SOW confirmation. Tokens: spring, lps, buec, ut, utad, lpsy, ice_retest.',
    'string', true, 'normal', ARRAY['wyckoff','setup','sos','sow']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','wyckoff.setup.require_sos_sow_trigger','true'::jsonb,'Gate Phase-D setups behind SOS/SOW confirmation.'),
    ('detection','wyckoff.setup.sos_sow_max_bars_ago','50'::jsonb,'Max bars between SOS/SOW and setup trigger.'),
    ('detection','wyckoff.setup.sos_sow_required_for','"lps,buec,lpsy,ice_retest"'::jsonb,'Setups requiring SOS/SOW confirmation (CSV).')
ON CONFLICT (module, config_key) DO NOTHING;
