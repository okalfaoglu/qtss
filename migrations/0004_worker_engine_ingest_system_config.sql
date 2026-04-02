-- Engine symbol market_bars ingest loop: prefer `system_config` (env fallback; `QTSS_CONFIG_ENV_OVERRIDES=1` → env wins).
INSERT INTO system_config (module, config_key, value, description)
VALUES
    (
        'worker',
        'engine_ingest_tick_secs',
        '{"secs":180}'::jsonb,
        'engine_symbol_ingest_loop sleep between full passes (seconds); env QTSS_ENGINE_INGEST_TICK_SECS; min 60 in worker.'
    ),
    (
        'worker',
        'engine_ingest_min_bars',
        '{"value":2000}'::jsonb,
        'REST backfill target floor per engine_symbols row (market_bars count); env QTSS_ENGINE_INGEST_MIN_BARS; clamp 120–50000.'
    ),
    (
        'worker',
        'engine_ingest_gap_window',
        '{"value":2000}'::jsonb,
        'Trailing bar count for gap scan (newest window); env QTSS_ENGINE_INGEST_GAP_WINDOW; clamp 100–20000.'
    )
ON CONFLICT (module, config_key) DO NOTHING;
