-- Faz 9.7.2 — Extra config keys for the bookTicker price watcher.
-- The 9.7.0 batch (migration 0136) seeded `price_watcher.enabled`,
-- `price_watcher.stream`, `price_watcher.max_active` and
-- `price_watcher.health_persist_min_band_delta`. This patch adds the
-- tuning knobs the WS loop reads every reconnect.
--
-- CLAUDE.md #2 — no hardcoded constants in Rust; every value below
-- has a default here and is live-editable via the Config Editor.

SELECT _qtss_register_key(
    'price_watcher.symbols_refresh_secs', 'notify', 'price_watcher',
    'int', '60'::jsonb, 's',
    'How often (sec) to re-scan open setups for symbol-set drift. On change the WS reconnects with the new subscription list.',
    'number', false, 'normal', ARRAY['notify','faz97']
);

SELECT _qtss_register_key(
    'price_watcher.stale_tick_secs', 'notify', 'price_watcher',
    'int', '30'::jsonb, 's',
    'PriceTickStore entries older than this are purged on each refresh cycle. Guards against silently dead subscriptions.',
    'number', false, 'normal', ARRAY['notify','faz97']
);
