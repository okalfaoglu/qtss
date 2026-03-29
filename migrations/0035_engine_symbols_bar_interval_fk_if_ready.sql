-- 0034, `bar_intervals` henüz yokken uygulandıysa `bar_interval_id` eklenmemiş olabilir.
-- `bar_intervals` oluşturulduktan sonra (0013 içeriği veya el ile) bu migrasyon idempotent tamamlar.

DO $$
BEGIN
  IF to_regclass('public.engine_symbols') IS NOT NULL
     AND to_regclass('public.bar_intervals') IS NOT NULL THEN
    ALTER TABLE engine_symbols
      ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL;
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_bar_interval_id ON engine_symbols (bar_interval_id);
  END IF;
END $$;
