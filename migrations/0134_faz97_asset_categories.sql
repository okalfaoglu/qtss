-- Faz 9.7.0 — Asset category taxonomy
--
-- User-facing cards display a high-level asset category tag
-- (MEGA CAP / KRIPTO / FOREX / VADELI / ...) instead of raw venue
-- classification. The taxonomy is mirrored from the 13-row reference
-- the user supplied.
--
-- `symbol_category_map` allows manual override per (exchange, symbol).
-- Unmapped symbols fall back to a config-driven default per venue-class.

CREATE TABLE IF NOT EXISTS asset_categories (
    id          SMALLINT PRIMARY KEY,
    code        TEXT NOT NULL UNIQUE,    -- MEGA_CAP, KRIPTO, FOREX...
    label_tr    TEXT NOT NULL,
    label_en    TEXT,
    description TEXT,
    display_order SMALLINT NOT NULL DEFAULT 0
);

INSERT INTO asset_categories (id, code, label_tr, description, display_order) VALUES
    (1,  'MEGA_CAP',    'MEGA CAP',    'Endeks ağırlıklı dev şirketler',  1),
    (2,  'LARGE_CAP',   'LARGE CAP',   'Büyük, güçlü bilanço',            2),
    (3,  'MID_CAP',     'MID CAP',     'Orta ölçekli, sektör lideri',     3),
    (4,  'GROWTH',      'GROWTH',      'Büyüme hikayesi, ivmeli',         4),
    (5,  'SMALL_CAP',   'SMALL CAP',   'Küçük ölçekli, niş',              5),
    (6,  'SPECULATIVE', 'SPECULATIVE', 'Momentum/haber odaklı',           6),
    (7,  'MICRO_PENNY', 'MICRO/PENNY', 'Çok düşük fiyat, aşırı volatil',  7),
    (8,  'HOLDING',     'HOLDİNG',     'Yatırım holdingleri, NAD takibi', 8),
    (9,  'ENDEKS',      'ENDEKS',      'Borsa endeksleri',                9),
    (10, 'EMTIA',       'EMTİA',       'Altın, gümüş, petrol',           10),
    (11, 'FOREX',       'FOREX',       'Döviz çiftleri',                 11),
    (12, 'VADELI',      'VADELİ',      'VİOP sözleşmeleri',              12),
    (13, 'KRIPTO',      'KRİPTO',      'Dijital varlıklar',              13)
ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS symbol_category_map (
    exchange    TEXT NOT NULL,
    symbol      TEXT NOT NULL,
    category_id SMALLINT NOT NULL REFERENCES asset_categories(id),
    source      TEXT NOT NULL DEFAULT 'auto',  -- auto | manual | rule
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (exchange, symbol)
);

CREATE INDEX IF NOT EXISTS idx_symbol_category_map_cat
    ON symbol_category_map (category_id);

COMMENT ON TABLE symbol_category_map IS
  'Exchange+symbol to asset_category. "source" is auto (rule engine), manual (operator override), or rule (config pattern).';

-- Config keys for the auto-classifier.
SELECT _qtss_register_key(
    'category.crypto.mega_cap_top_n', 'notify', 'category',
    'int', '10'::jsonb, '',
    'Crypto symbols ranked <= this (by market cap) map to MEGA_CAP.',
    'number', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'category.crypto.large_cap_top_n', 'notify', 'category',
    'int', '50'::jsonb, '',
    'Ranked <= this (and > mega_cap_top_n) map to LARGE_CAP.',
    'number', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'category.crypto.mid_cap_top_n', 'notify', 'category',
    'int', '200'::jsonb, '',
    'Ranked <= this (and > large_cap_top_n) map to MID_CAP.',
    'number', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'category.crypto.small_cap_top_n', 'notify', 'category',
    'int', '1000'::jsonb, '',
    'Ranked <= this (and > mid_cap_top_n) map to SMALL_CAP. Remainder -> MICRO_PENNY.',
    'number', false, 'normal', ARRAY['notify','faz97']
);
SELECT _qtss_register_key(
    'category.crypto.futures_override', 'notify', 'category',
    'bool', 'true'::jsonb, '',
    'Perpetual/futures symbols map to VADELI regardless of market-cap rank.',
    'bool', false, 'normal', ARRAY['notify','faz97']
);
