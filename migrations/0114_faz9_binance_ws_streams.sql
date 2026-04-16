-- 0114_faz9_binance_ws_streams.sql
--
-- Faz 9.0.0 — Binance public WS feature stream toggles/timings.
--
-- Worker `binance_public_ws` iki döngü açar:
--   * liquidation_stream_loop  (`@forceOrder`)
--   * aggtrade_cvd_loop        (`@aggTrade`)
--
-- Her iki döngü buffer'ı periyodik olarak `data_snapshots`'e yazar
--   * `binance_liquidations_<pair_lower>`
--   * `binance_cvd_<pair_lower>`
-- CLAUDE.md #2: tick/window/bucket parametreleri config tablosunda.

SELECT _qtss_register_key(
    'liquidation_stream.enabled','binance_ws','binance_ws','bool',
    'true'::jsonb, '',
    'Binance futures forceOrder WS loop — liquidation events → data_snapshots(binance_liquidations_*).',
    'bool', false, 'normal', ARRAY['binance','ws','liquidation']);

SELECT _qtss_register_key(
    'liquidation_stream.flush_secs','binance_ws','binance_ws','int',
    '30'::jsonb, 'seconds',
    'Flush frequency for liquidation buffer → data_snapshots.',
    'number', false, 'normal', ARRAY['binance','ws','liquidation']);

SELECT _qtss_register_key(
    'liquidation_stream.window_secs','binance_ws','binance_ws','int',
    '3600'::jsonb, 'seconds',
    'Rolling window retained in data_snapshots for liquidation events.',
    'number', false, 'normal', ARRAY['binance','ws','liquidation']);

SELECT _qtss_register_key(
    'aggtrade_cvd.enabled','binance_ws','binance_ws','bool',
    'true'::jsonb, '',
    'Binance futures aggTrade WS loop — CVD bucket ingestion.',
    'bool', false, 'normal', ARRAY['binance','ws','cvd']);

SELECT _qtss_register_key(
    'aggtrade_cvd.flush_secs','binance_ws','binance_ws','int',
    '30'::jsonb, 'seconds',
    'Flush frequency for CVD bucket buffer → data_snapshots.',
    'number', false, 'normal', ARRAY['binance','ws','cvd']);

SELECT _qtss_register_key(
    'aggtrade_cvd.window_secs','binance_ws','binance_ws','int',
    '3600'::jsonb, 'seconds',
    'Rolling window (seconds) retained per symbol in CVD snapshot.',
    'number', false, 'normal', ARRAY['binance','ws','cvd']);

SELECT _qtss_register_key(
    'aggtrade_cvd.bucket_secs','binance_ws','binance_ws','int',
    '60'::jsonb, 'seconds',
    'CVD bucket granularity (seconds).',
    'number', false, 'normal', ARRAY['binance','ws','cvd']);

SELECT _qtss_register_key(
    'aggtrade_cvd.max_symbols','binance_ws','binance_ws','int',
    '20'::jsonb, 'symbols',
    'Safety cap on combined aggTrade stream size (high msg volume).',
    'number', false, 'normal', ARRAY['binance','ws','cvd']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('binance_ws','liquidation_stream.enabled','true'::jsonb,'forceOrder WS master switch.'),
    ('binance_ws','liquidation_stream.flush_secs','30'::jsonb,'Liquidation buffer flush (secs).'),
    ('binance_ws','liquidation_stream.window_secs','3600'::jsonb,'Liquidation rolling window (secs).'),
    ('binance_ws','aggtrade_cvd.enabled','true'::jsonb,'aggTrade WS master switch.'),
    ('binance_ws','aggtrade_cvd.flush_secs','30'::jsonb,'CVD buffer flush (secs).'),
    ('binance_ws','aggtrade_cvd.window_secs','3600'::jsonb,'CVD rolling window (secs).'),
    ('binance_ws','aggtrade_cvd.bucket_secs','60'::jsonb,'CVD bucket size (secs).'),
    ('binance_ws','aggtrade_cvd.max_symbols','20'::jsonb,'CVD combined stream cap.')
ON CONFLICT (module, config_key) DO NOTHING;
