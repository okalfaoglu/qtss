-- Mum periyotları kataloğu (engine_symbols / market_bars ile ilişkilendirilebilir).
-- `code` Binance tarzı: 1m, 3m, 5m, 15m, 1h, 4h, 1d, ...

CREATE TABLE bar_intervals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL UNIQUE,
    label TEXT,
    duration_seconds INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT true,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_bar_intervals_active ON bar_intervals (is_active, sort_order);

COMMENT ON TABLE bar_intervals IS 'OHLC mum aralığı kataloğu; market_bars / engine_symbols FK ile tekrarlayan metin azaltılır.';

-- Binance senkronu: futures + contract_kind usdt_m (segment CHECK eski listede yoktu).
ALTER TABLE markets DROP CONSTRAINT IF EXISTS markets_segment_check;

INSERT INTO bar_intervals (code, label, duration_seconds, sort_order) VALUES
    ('1s', '1 saniye', 1, 5),
    ('1m', '1 dakika', 60, 10),
    ('3m', '3 dakika', 180, 15),
    ('5m', '5 dakika', 300, 20),
    ('15m', '15 dakika', 900, 30),
    ('30m', '30 dakika', 1800, 40),
    ('1h', '1 saat', 3600, 50),
    ('2h', '2 saat', 7200, 55),
    ('4h', '4 saat', 14400, 60),
    ('6h', '6 saat', 21600, 65),
    ('8h', '8 saat', 28800, 70),
    ('12h', '12 saat', 43200, 75),
    ('1d', '1 gün', 86400, 80),
    ('3d', '3 gün', 259200, 85),
    ('1w', '1 hafta', 604800, 90),
    ('1M', '1 ay', NULL, 100)
ON CONFLICT (code) DO NOTHING;
