-- Faz 9.8.14 — tick dispatcher loop config.
--
-- Polls PriceTickStore for every TickKey in LivePositionStore, runs
-- guard dispatch, persists actionable outcomes to
-- liquidation_guard_events / position_scale_events, and keeps
-- live_positions.last_mark in sync.

SELECT _qtss_register_key(
    'risk.tick_dispatcher.enabled', 'risk', 'tick_dispatcher',
    'bool', 'true'::jsonb, '',
    'Enable tick dispatcher loop (guard fan-out on live_positions).',
    'toggle', false, 'high', ARRAY['risk','faz9814','tick']
);

SELECT _qtss_register_key(
    'risk.tick_dispatcher.eval_interval_ms', 'risk', 'tick_dispatcher',
    'int', '1000'::jsonb, '',
    'Milliseconds between guard evaluation sweeps across open positions.',
    'number', false, 'normal', ARRAY['risk','faz9814','tick']
);

SELECT _qtss_register_key(
    'risk.tick_dispatcher.hydrate_interval_secs', 'risk', 'tick_dispatcher',
    'int', '60'::jsonb, '',
    'Seconds between re-hydrations of LivePositionStore from the DB.',
    'number', false, 'normal', ARRAY['risk','faz9814','tick']
);

SELECT _qtss_register_key(
    'risk.tick_dispatcher.stale_mark_secs', 'risk', 'tick_dispatcher',
    'int', '30'::jsonb, '',
    'Skip evaluation when the PriceTickStore entry is older than this.',
    'number', false, 'normal', ARRAY['risk','faz9814','tick']
);
