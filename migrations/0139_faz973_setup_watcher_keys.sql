-- Faz 9.7.3 — SetupWatcher loop config keys.
-- The watcher is disabled by default; operators flip the switch in
-- the Config Editor once migrations 0133-0138 are verified on the
-- target environment.

SELECT _qtss_register_key(
    'setup_watcher.enabled', 'notify', 'setup_watcher',
    'bool', 'false'::jsonb, '',
    'Enable the SetupWatcher loop (lifecycle dispatch + Health Score). Requires price_watcher.enabled=true to have any data.',
    'bool', true, 'normal', ARRAY['notify','faz97']
);

SELECT _qtss_register_key(
    'setup_watcher.tick_secs', 'notify', 'setup_watcher',
    'int', '5'::jsonb, 's',
    'How often the watcher sweeps open setups against the PriceTickStore.',
    'number', false, 'normal', ARRAY['notify','faz97']
);
