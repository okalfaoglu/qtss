-- 0109_wyckoff_invalidation_sweeper.sql
--
-- P7.6 — Reverse-trigger / failed-setup detection. Background loop
-- closes open Wyckoff setups whose tight or structural SL has been
-- breached by the latest bar close.

SELECT _qtss_register_key(
    'wyckoff.invalidation.enabled','setup','detection','bool',
    'true'::jsonb, 'flag',
    'Enable the Wyckoff setup invalidation sweeper loop.',
    'boolean', true, 'normal', ARRAY['wyckoff','setup','invalidation']);

SELECT _qtss_register_key(
    'wyckoff.invalidation.interval_seconds','setup','detection','int',
    '60'::jsonb, 'seconds',
    'Tick interval for the Wyckoff setup invalidation sweeper. Default 60s.',
    'number', true, 'normal', ARRAY['wyckoff','setup','invalidation']);

SELECT _qtss_register_key(
    'wyckoff.invalidation.breach_slack_ratio','setup','detection','float',
    '0.0'::jsonb, 'ratio',
    'Slack tolerance around the SL as a ratio of last close (e.g. 0.0005 = 5 bps) to suppress single-tick noise. 0 = hard breach.',
    'number', true, 'normal', ARRAY['wyckoff','setup','invalidation']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','wyckoff.invalidation.enabled','true'::jsonb,'Enable the Wyckoff invalidation sweeper.'),
    ('detection','wyckoff.invalidation.interval_seconds','60'::jsonb,'Sweeper tick interval.'),
    ('detection','wyckoff.invalidation.breach_slack_ratio','0.0'::jsonb,'Slack around SL breach.')
ON CONFLICT (module, config_key) DO NOTHING;
