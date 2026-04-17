-- Faz 9.7.6 — X outbox handler + publisher loop config keys.
-- Handler is enabled by default (enqueues to outbox); publisher loop
-- ships off by default so ops can verify queue contents before
-- flipping the X posting switch.

SELECT _qtss_register_key(
    'x_outbox_handler.enabled', 'notify', 'x',
    'bool', 'true'::jsonb, '',
    'Attach the XOutboxHandler to the SetupWatcher router. When false, lifecycle events are not enqueued to x_outbox.',
    'bool', true, 'normal', ARRAY['notify','faz97','x']
);

SELECT _qtss_register_key(
    'x.publisher.enabled', 'notify', 'x',
    'bool', 'false'::jsonb, '',
    'Enable the x_outbox publisher loop. Off by default so operators can verify queue contents before posting to X.',
    'bool', true, 'normal', ARRAY['notify','faz97','x']
);

SELECT _qtss_register_key(
    'x.publisher.batch_limit', 'notify', 'x',
    'int', '10'::jsonb, '',
    'Max rows the publisher drains per tick.',
    'number', false, 'normal', ARRAY['notify','faz97','x']
);

SELECT _qtss_register_key(
    'x.publisher.daily_cap', 'notify', 'x',
    'int', '50'::jsonb, '',
    'Maximum X posts per UTC day. Publisher short-circuits once the count is reached.',
    'number', false, 'normal', ARRAY['notify','faz97','x']
);
