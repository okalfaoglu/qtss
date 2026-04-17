-- 0131_faz9_orderbook_enrichment.sql
--
-- Faz 9.6 — Orderbook depth stream + feature enrichment.
--
-- Adds config keys for the Binance @depth20@100ms WS stream and
-- the OrderbookSource feature extractor.

-- ─── config keys ─────────────────────────────────────────────────

SELECT _qtss_register_key(
    'binance_ws.depth_stream.enabled',
    'binance_ws',
    'depth_stream',
    'bool',
    'true'::jsonb,
    '',
    'Enable the orderbook depth WS stream (@depth20@100ms).',
    'bool',
    false,
    'normal',
    ARRAY['binance_ws','orderbook','faz9']
);

SELECT _qtss_register_key(
    'binance_ws.depth_stream.flush_secs',
    'binance_ws',
    'depth_stream',
    'int',
    '30'::jsonb,
    '',
    'How often (seconds) to flush orderbook depth summaries to data_snapshots.',
    'secs',
    false,
    'normal',
    ARRAY['binance_ws','orderbook','faz9']
);

SELECT _qtss_register_key(
    'binance_ws.depth_stream.levels',
    'binance_ws',
    'depth_stream',
    'int',
    '20'::jsonb,
    '',
    'Number of orderbook levels received from Binance depth stream.',
    'levels',
    false,
    'normal',
    ARRAY['binance_ws','orderbook','faz9']
);

SELECT _qtss_register_key(
    'orderbook.imbalance_levels',
    'features',
    'orderbook',
    'int',
    '10'::jsonb,
    '',
    'Number of top levels for bid/ask imbalance calc.',
    'levels',
    false,
    'normal',
    ARRAY['features','orderbook','faz9']
);

SELECT _qtss_register_key(
    'orderbook.depth_pct',
    'features',
    'orderbook',
    'float',
    '0.02'::jsonb,
    '',
    'Price distance (fraction) for depth aggregation.',
    'fraction',
    false,
    'normal',
    ARRAY['features','orderbook','faz9']
);
