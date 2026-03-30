-- FAZ 11.7 — kill_switch DB sync + PnL poll intervals in system_config (non-secret).

INSERT INTO system_config (module, config_key, value, description)
VALUES
    (
        'worker',
        'kill_switch_db_sync_tick_secs',
        '{"secs":5}'::jsonb,
        'Poll app_config kill_switch_trading_halted for in-process halt flag; env QTSS_KILL_SWITCH_DB_SYNC_SECS; min 2.'
    ),
    (
        'worker',
        'kill_switch_pnl_poll_tick_secs',
        '{"secs":60}'::jsonb,
        'kill_switch_loop PnL check interval when QTSS_KILL_SWITCH_ENABLED; env QTSS_KILL_SWITCH_TICK_SECS; min 15.'
    )
ON CONFLICT (module, config_key) DO NOTHING;
