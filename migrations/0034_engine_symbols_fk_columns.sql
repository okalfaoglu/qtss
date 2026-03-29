-- Bazı kurulumlarda 0014 uygulanmamış; `engine_symbols.exchange_id` vb. eksik kalınca worker sorguları kırılır.
-- `bar_intervals` henüz yoksa (0013 atlanmış DB’ler) tek ALTER içinde REFERENCES tüm ifadeyi düşürürdü; bu yüzden
-- `exchange_id` / `market_id` / `instrument_id` ayrı, `bar_interval_id` yalnızca `bar_intervals` varsa eklenir.

DO $$
BEGIN
  IF to_regclass('public.engine_symbols') IS NOT NULL THEN
    ALTER TABLE engine_symbols
      ADD COLUMN IF NOT EXISTS exchange_id UUID REFERENCES exchanges (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS market_id UUID REFERENCES markets (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL;

    CREATE INDEX IF NOT EXISTS idx_engine_symbols_exchange_id ON engine_symbols (exchange_id);
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_market_id ON engine_symbols (market_id);
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_instrument_id ON engine_symbols (instrument_id);

    IF to_regclass('public.bar_intervals') IS NOT NULL THEN
      ALTER TABLE engine_symbols
        ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL;
      CREATE INDEX IF NOT EXISTS idx_engine_symbols_bar_interval_id ON engine_symbols (bar_interval_id);
    END IF;
  END IF;
END $$;
