-- FAZ 11.7 — paper / live position notify poll intervals in `system_config` (non-secret).

INSERT INTO system_config (module, config_key, value, description)
VALUES
    (
        'worker',
        'paper_position_notify_tick_secs',
        '{"secs":30}'::jsonb,
        'Paper fill notify loop interval (seconds); env QTSS_NOTIFY_POSITION_TICK_SECS; min 10 in worker.'
    ),
    (
        'worker',
        'live_position_notify_tick_secs',
        '{"secs":45}'::jsonb,
        'Live fill notify loop interval (seconds); env QTSS_NOTIFY_LIVE_TICK_SECS; min 15 in worker.'
    )
ON CONFLICT (module, config_key) DO NOTHING;
