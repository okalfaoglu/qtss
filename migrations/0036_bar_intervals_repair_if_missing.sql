-- Telafi: `_sqlx_migrations` içinde 0013 uygulanmış görünüp `public.bar_intervals` yoksa (eski dosya, el ile silme, yarım transaction).
-- Uygulanmış 0013 içeriğini değiştirmeyin; bu dosya idempotent tamamlar. Yeni kurulumlarda çoğu adım no-op.

ALTER TABLE markets DROP CONSTRAINT IF EXISTS markets_segment_check;

ALTER TABLE market_bars
    ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS bar_interval_id UUID;

CREATE TABLE IF NOT EXISTS bar_intervals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL,
    label TEXT,
    duration_seconds INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT bar_intervals_code_key UNIQUE (code)
);

DO $$
BEGIN
    ALTER TABLE market_bars
        ADD CONSTRAINT market_bars_bar_interval_id_fkey FOREIGN KEY (bar_interval_id) REFERENCES bar_intervals (id) ON DELETE SET NULL;
EXCEPTION
    WHEN duplicate_object THEN NULL;
END
$$;

INSERT INTO bar_intervals (code, label, duration_seconds, sort_order, is_active, metadata)
VALUES
    ('1m', '1 minute', 60, 10, TRUE, '{}'),
    ('3m', '3 minutes', 180, 20, TRUE, '{}'),
    ('5m', '5 minutes', 300, 30, TRUE, '{}'),
    ('15m', '15 minutes', 900, 40, TRUE, '{}'),
    ('30m', '30 minutes', 1800, 50, TRUE, '{}'),
    ('1h', '1 hour', 3600, 60, TRUE, '{}'),
    ('2h', '2 hours', 7200, 70, TRUE, '{}'),
    ('4h', '4 hours', 14400, 80, TRUE, '{}'),
    ('1d', '1 day', 86400, 90, TRUE, '{}')
ON CONFLICT (code) DO NOTHING;

INSERT INTO bar_intervals (code, label, duration_seconds, sort_order, is_active, metadata) VALUES
    ('1s', '1 saniye', 1, 5, TRUE, '{}'),
    ('6h', '6 saat', 21600, 65, TRUE, '{}'),
    ('8h', '8 saat', 28800, 70, TRUE, '{}'),
    ('12h', '12 saat', 43200, 75, TRUE, '{}'),
    ('3d', '3 gün', 259200, 85, TRUE, '{}'),
    ('1w', '1 hafta', 604800, 90, TRUE, '{}'),
    ('1M', '1 ay', NULL, 100, TRUE, '{}')
ON CONFLICT (code) DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_bar_intervals_active ON bar_intervals (is_active, sort_order);

COMMENT ON TABLE bar_intervals IS 'OHLC mum aralığı kataloğu; market_bars / engine_symbols FK ile tekrarlayan metin azaltılır.';

UPDATE app_config
SET value = jsonb_set(
    value,
    '{scanning}',
    coalesce(value->'scanning', '{}'::jsonb)
      || '{"ratio_diff_enabled": false, "ratio_diff_max": 1.0}'::jsonb,
    true
  ),
  updated_at = now()
WHERE key = 'acp_chart_patterns'
  AND (value->'scanning'->'ratio_diff_enabled' IS NULL);

UPDATE market_bars mb
SET bar_interval_id = bi.id
FROM bar_intervals bi
WHERE mb.bar_interval_id IS NULL
  AND LOWER(TRIM(mb.interval)) = LOWER(bi.code);

-- 0035, `bar_intervals` yokken uygulanmışsa `bar_interval_id` hiç eklenmemiş olabilir; önce kolon, sonra doldurma.
DO $$
BEGIN
  IF to_regclass('public.engine_symbols') IS NOT NULL
     AND to_regclass('public.bar_intervals') IS NOT NULL THEN
    ALTER TABLE engine_symbols
      ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL;
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_bar_interval_id ON engine_symbols (bar_interval_id);
  END IF;
END $$;

DO $$
BEGIN
  IF to_regclass('public.engine_symbols') IS NOT NULL THEN
    UPDATE engine_symbols es
    SET bar_interval_id = bi.id
    FROM bar_intervals bi
    WHERE es.bar_interval_id IS NULL
      AND LOWER(TRIM(es.interval)) = LOWER(bi.code);
  END IF;
END $$;
