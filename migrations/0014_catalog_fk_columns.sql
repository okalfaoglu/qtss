-- engine_symbols / market_bars → katalog FK (metin kolonları geriye dönük uyumluluk için kalır).

DO $$
BEGIN
  IF to_regclass('public.engine_symbols') IS NOT NULL THEN
    ALTER TABLE engine_symbols
      ADD COLUMN IF NOT EXISTS exchange_id UUID REFERENCES exchanges (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS market_id UUID REFERENCES markets (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL;
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_exchange_id ON engine_symbols (exchange_id);
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_market_id ON engine_symbols (market_id);
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_instrument_id ON engine_symbols (instrument_id);
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_bar_interval_id ON engine_symbols (bar_interval_id);
  END IF;

  IF to_regclass('public.market_bars') IS NOT NULL THEN
    ALTER TABLE market_bars
      ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL;
    CREATE INDEX IF NOT EXISTS idx_market_bars_instrument_interval_time
      ON market_bars (instrument_id, bar_interval_id, open_time DESC)
      WHERE instrument_id IS NOT NULL AND bar_interval_id IS NOT NULL;
  END IF;
END $$;

-- bar_interval_id doldur (metin interval ile)
UPDATE market_bars mb
SET bar_interval_id = bi.id
FROM bar_intervals bi
WHERE mb.bar_interval_id IS NULL
  AND LOWER(TRIM(mb.interval)) = LOWER(bi.code);

-- instrument_id: borsa + segment eşlemesi (worker segment: spot | futures)
UPDATE market_bars mb
SET instrument_id = i.id
FROM instruments i
INNER JOIN markets m ON m.id = i.market_id
INNER JOIN exchanges e ON e.id = m.exchange_id
WHERE mb.instrument_id IS NULL
  AND LOWER(TRIM(mb.exchange)) = LOWER(e.code)
  AND (
    (LOWER(TRIM(mb.segment)) = 'spot' AND m.segment = 'spot' AND (m.contract_kind = '' OR m.contract_kind IS NULL))
    OR (
      LOWER(TRIM(mb.segment)) IN ('futures', 'usdt_futures', 'fapi')
      AND m.segment = 'futures'
      AND m.contract_kind = 'usdt_m'
    )
  )
  AND UPPER(TRIM(mb.symbol)) = UPPER(i.native_symbol);

UPDATE engine_symbols es
SET bar_interval_id = bi.id
FROM bar_intervals bi
WHERE es.bar_interval_id IS NULL
  AND LOWER(TRIM(es.interval)) = LOWER(bi.code);

UPDATE engine_symbols es
SET exchange_id = e.id
FROM exchanges e
WHERE es.exchange_id IS NULL
  AND LOWER(TRIM(es.exchange)) = LOWER(e.code);

UPDATE engine_symbols es
SET market_id = m.id
FROM markets m
INNER JOIN exchanges e ON e.id = m.exchange_id
WHERE es.market_id IS NULL
  AND es.exchange_id = e.id
  AND (
    (LOWER(TRIM(es.segment)) = 'spot' AND m.segment = 'spot' AND (m.contract_kind = '' OR m.contract_kind IS NULL))
    OR (
      LOWER(TRIM(es.segment)) IN ('futures', 'usdt_futures', 'fapi')
      AND m.segment = 'futures'
      AND m.contract_kind = 'usdt_m'
    )
  );

UPDATE engine_symbols es
SET instrument_id = i.id
FROM instruments i
WHERE es.instrument_id IS NULL
  AND es.market_id = i.market_id
  AND UPPER(TRIM(es.symbol)) = UPPER(i.native_symbol);

-- ACP: üst çubuk TF değişince otomatik kanal taraması (varsayılan kapalı).

UPDATE app_config
SET value = jsonb_set(
    value,
    '{scanning}',
    coalesce(value->'scanning', '{}'::jsonb)
      || '{"auto_scan_on_timeframe_change": false}'::jsonb,
    true
  ),
  updated_at = now()
WHERE key = 'acp_chart_patterns'
  AND (value->'scanning'->'auto_scan_on_timeframe_change' IS NULL);
