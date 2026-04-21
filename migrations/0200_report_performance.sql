-- 0200_report_performance.sql
--
-- Faz 15.R öncesi raporlama altyapısı — QTSK aylık/haftalık/günlük/yıllık
-- performans sayfası için gerekli iki eksik parça:
--
--   1. `report_exchange_class` — exchange string'ini varlık sınıfına
--      eşler (crypto | nasdaq | bist). Rapor sayfası bu sınıfa göre
--      sekmelere ayrılır; Binance USDT-perp + BIST Spot + NASDAQ Cash
--      aynı ekranda karıştırılmaz. Tablo GUI'den düzenlenebilir, yeni
--      borsa eklemek migration gerektirmez (CLAUDE.md #2).
--
--   2. `config_schema` seed'leri — sanal cüzdan hesaplamalarının
--      parametreleri. Setup Engine + paper-trading (Faz 14.B + 15)
--      devreye girene kadar rapor bu sabitler üzerinden "varsayımsal
--      equity" hesaplar; Faz 15 ile birlikte gerçek paper ledger'a
--      bağlanır ve bu default'lar fallback olur.

BEGIN;

-- ─── 1. exchange → asset class eşlemesi ────────────────────────────────
CREATE TABLE IF NOT EXISTS report_exchange_class (
    exchange    TEXT PRIMARY KEY,
    class       TEXT NOT NULL CHECK (class IN ('crypto','nasdaq','bist')),
    display     TEXT,                  -- UI'de gösterilecek kısa ad
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed — veri tabanında halen "binance", "bist", "nasdaq" lower-case
-- normalize ediliyor (bkz. qtss_v2_detections.exchange). Yeni venue
-- eklendiğinde GUI'den insert atılır.
INSERT INTO report_exchange_class (exchange, class, display) VALUES
    ('binance',          'crypto', 'Binance'),
    ('binance_futures',  'crypto', 'Binance Futures'),
    ('binance_spot',     'crypto', 'Binance Spot'),
    ('bybit',            'crypto', 'Bybit'),
    ('okx',              'crypto', 'OKX'),
    ('coinbase',         'crypto', 'Coinbase'),
    ('bist',             'bist',   'BIST'),
    ('nasdaq',           'nasdaq', 'NASDAQ')
ON CONFLICT (exchange) DO NOTHING;

-- ─── 2. Paper-trading rapor parametreleri ──────────────────────────────
-- Raporlar sanal cüzdan mantığıyla çalışır: her kapanan işlemde
-- allocation_pct × starting_equity kadar sermaye riske giriyor gibi
-- hesaplanır, pnl_pct bu tutara uygulanır, commission_bps × 2
-- (entry+exit) kesilir. Faz 15 gerçek ledger yazana kadar bu
-- idealize edilmiş eğri "referans" olarak kullanılacak.
INSERT INTO config_schema (
    key, category, subcategory, value_type, default_value,
    unit, description, ui_widget, requires_restart, sensitivity,
    introduced_in, tags
) VALUES
('report.paper.starting_equity.crypto',  'report', 'paper', 'float', '10000'::jsonb,
    'USDT', 'Crypto raporu için başlangıç sermayesi (sanal).',
    'number', false, 'normal', '0200', ARRAY['faz15','report']::TEXT[]),
('report.paper.starting_equity.nasdaq',  'report', 'paper', 'float', '10000'::jsonb,
    'USD',  'NASDAQ raporu için başlangıç sermayesi (sanal).',
    'number', false, 'normal', '0200', ARRAY['faz15','report']::TEXT[]),
('report.paper.starting_equity.bist',    'report', 'paper', 'float', '1500000'::jsonb,
    'TL',   'BIST raporu için başlangıç sermayesi (sanal). 1.5M TL default.',
    'number', false, 'normal', '0200', ARRAY['faz15','report']::TEXT[]),
('report.paper.allocation_pct',          'report', 'paper', 'float', '0.10'::jsonb,
    'ratio', 'İşlem başına sermayenin yüzde kaçı riske girer (0.10 = %10).',
    'slider', false, 'normal', '0200', ARRAY['faz15','report']::TEXT[]),
('report.paper.commission_bps.crypto',   'report', 'paper', 'float', '10'::jsonb,
    'bps',  'Crypto spot maker+taker ortalama komisyon (10 bps = %0.10).',
    'number', false, 'normal', '0200', ARRAY['faz15','report']::TEXT[]),
('report.paper.commission_bps.nasdaq',   'report', 'paper', 'float', '2'::jsonb,
    'bps',  'NASDAQ komisyon tahmini (2 bps = %0.02).',
    'number', false, 'normal', '0200', ARRAY['faz15','report']::TEXT[]),
('report.paper.commission_bps.bist',     'report', 'paper', 'float', '15'::jsonb,
    'bps',  'BIST komisyon + BSMV tahmini (15 bps).',
    'number', false, 'normal', '0200', ARRAY['faz15','report']::TEXT[])
ON CONFLICT (key) DO NOTHING;

COMMIT;
