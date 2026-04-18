-- Faz 9.8.12 — trainer cron + sidecar probe config.
--
-- Registers three knobs the trainer_cron loop reads. Command defaults
-- to the Python entry point; override if the trainer is shipped as a
-- systemd unit or a container image (e.g. 'docker exec qtss-trainer
-- qtss-trainer train').

SELECT _qtss_register_key(
    'trainer.cron.enabled', 'ai', 'trainer_cron',
    'bool', 'true'::jsonb, '',
    'Enable periodic trainer invocation from the worker.',
    'toggle', false, 'normal', ARRAY['ai','faz9812','trainer']
);

SELECT _qtss_register_key(
    'trainer.cron.interval_hours', 'ai', 'trainer_cron',
    'int', '168'::jsonb, '',
    'Hours between trainer invocations (default 168 = weekly).',
    'number', false, 'normal', ARRAY['ai','faz9812','trainer']
);

SELECT _qtss_register_key(
    'trainer.cron.command', 'ai', 'trainer_cron',
    'string', '"python -m qtss_trainer train"'::jsonb, '',
    'Shell command executed by trainer cron (no shell expansion).',
    'text', false, 'high', ARRAY['ai','faz9812','trainer']
);
