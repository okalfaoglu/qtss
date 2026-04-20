-- 0197_symbol_profile.sql
--
-- Faz 14.A / A1 — Symbol Intelligence & Position Sizer.
--
-- Her varlık (crypto / BIST / NASDAQ) için tek satırlık profil:
--   * kategori / risk tier (MEGA / LARGE / MID / SMALL / SPEC / MICRO / …)
--   * fundamentals snapshot (market_cap, free_float, avg_volume)
--   * venue-özel kısıtlar (lot_size, tick_size, min_notional)
--   * türetilmiş skorlar (fundamental / liquidity / volatility, 0..100)
--   * manuel override flag'i (operatör DB üstünden elle sabitleyebilir)
--
-- Kategoriler görseldeki 13 maddeyi birebir yansıtır + kripto-özel
-- üç ek (stablecoin, defi, meme) katman olarak `sector` kolonunda
-- tutulur; ana `category` alanı sermaye tahsisat mantığını domine eder.
--
-- Tek tablo, hepsi upsert. Günlük `symbol_catalog_refresh` binary'si
-- Binance exchangeInfo + CoinGecko birleşik upsert ile besler (A2+A3).

BEGIN;

CREATE TABLE IF NOT EXISTS qtss_symbol_profile (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange         TEXT NOT NULL,            -- binance | bist | nasdaq | …
    symbol           TEXT NOT NULL,
    asset_class      TEXT NOT NULL,            -- crypto | equity | commodity | fx | futures | index
    category         TEXT NOT NULL,            -- mega_cap | large_cap | mid_cap | small_cap |
                                               -- growth | speculative | micro_penny | holding |
                                               -- endeks | emtia | forex | vadeli | kripto
    risk_tier        TEXT NOT NULL,            -- core | balanced | growth | speculative | extreme
    sector           TEXT,                     -- banka, holding, L1, defi, meme, AI, … serbest metin
    country          TEXT,                     -- TR | US | GLOBAL

    -- Fundamentals snapshot (nullable; crypto için çoğu anlamsız)
    market_cap_usd       NUMERIC,
    circulating_supply   NUMERIC,              -- crypto
    free_float_pct       NUMERIC,              -- equity
    avg_daily_vol_usd    NUMERIC,              -- 30-gün ort. dolar hacmi
    price_usd            NUMERIC,              -- son snapshot (sıralama için kolaylık)

    -- Venue kısıtları
    lot_size         NUMERIC,                  -- BIST: lot, crypto/NASDAQ: genelde 1 veya step_size
    tick_size        NUMERIC,
    min_notional     NUMERIC,
    step_size        NUMERIC,                  -- crypto (Binance LOT_SIZE filter)

    -- Türetilmiş skorlar 0..100
    fundamental_score SMALLINT,
    liquidity_score   SMALLINT,
    volatility_score  SMALLINT,

    -- Manuel override: true ise catalog_refresh bu satırı değiştirmez.
    manual_override  BOOLEAN NOT NULL DEFAULT FALSE,
    notes            TEXT,

    source           TEXT,                     -- coingecko+binance | yfinance | kap | manual
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT qtss_symbol_profile_uniq UNIQUE (exchange, symbol),
    CONSTRAINT qtss_symbol_profile_category_chk CHECK (category IN (
        'mega_cap','large_cap','mid_cap','small_cap',
        'growth','speculative','micro_penny','holding',
        'endeks','emtia','forex','vadeli','kripto'
    )),
    CONSTRAINT qtss_symbol_profile_tier_chk CHECK (risk_tier IN (
        'core','balanced','growth','speculative','extreme'
    )),
    CONSTRAINT qtss_symbol_profile_asset_chk CHECK (asset_class IN (
        'crypto','equity','commodity','fx','futures','index'
    ))
);

CREATE INDEX IF NOT EXISTS ix_qtss_symbol_profile_exchange
    ON qtss_symbol_profile (exchange);
CREATE INDEX IF NOT EXISTS ix_qtss_symbol_profile_tier
    ON qtss_symbol_profile (risk_tier);
CREATE INDEX IF NOT EXISTS ix_qtss_symbol_profile_category
    ON qtss_symbol_profile (category);
CREATE INDEX IF NOT EXISTS ix_qtss_symbol_profile_updated
    ON qtss_symbol_profile (updated_at DESC);

COMMENT ON TABLE qtss_symbol_profile IS
    'Faz 14 — per-symbol intelligence snapshot. Risk tier + fundamentals drive the position sizer.';
COMMENT ON COLUMN qtss_symbol_profile.risk_tier IS
    'Drives max_alloc_pct cap via qtss_symbol_intel.tier_cap config (see 0197 seed).';
COMMENT ON COLUMN qtss_symbol_profile.manual_override IS
    'When TRUE, catalog_refresh leaves this row untouched so operators can pin exotic tiers manually.';

-- ───────────────────────────────────────────────────────────────────────
-- Günlük piyasa / sektör rejim snapshot'ı (A6'da dolacak).
-- Tek tablo: exchange × sector × gün, ama sector='*' satırı piyasa-geneli
-- rejimi temsil eder (BIST100, BTC.D + TOTAL2, S&P500).
-- ───────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS qtss_market_regime_daily (
    day              DATE NOT NULL,
    exchange         TEXT NOT NULL,
    sector           TEXT NOT NULL DEFAULT '*',  -- '*' = borsa geneli rollup
    regime           TEXT NOT NULL,              -- risk_on | neutral | risk_off | panic
    breadth_pct      NUMERIC,                    -- %52W-high (equity) / %BTC-üstü (crypto)
    momentum_20d     NUMERIC,                    -- sektör ya da indeks 20g momentum
    volatility_index NUMERIC,                    -- VIX / BTC.D ATR% / BIST100 ATR%
    dominant_trend   TEXT,                       -- up | down | chop
    source           TEXT,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (day, exchange, sector),
    CONSTRAINT qtss_market_regime_chk CHECK (regime IN (
        'risk_on','neutral','risk_off','panic'
    ))
);

COMMENT ON TABLE qtss_market_regime_daily IS
    'Faz 14 — günlük piyasa/sektör rejim skoru; sizer risk_pct üzerinde çarpan uygular.';

-- ───────────────────────────────────────────────────────────────────────
-- config_schema seed (CLAUDE.md #2 — hardcoded sabit yok).
-- Her tier için max pozisyon yüzdesi, regime çarpanları, min likidite
-- tabanı. Default'lar görseldeki mantığa (core ~%7, extreme ~%1) uyar.
-- ───────────────────────────────────────────────────────────────────────

CREATE OR REPLACE FUNCTION _qtss_sym_intel_seed(
    p_key         TEXT,
    p_value_type  TEXT,
    p_default     JSONB,
    p_unit        TEXT,
    p_description TEXT
) RETURNS VOID AS $$
BEGIN
    INSERT INTO config_schema (
        key, category, subcategory, value_type, default_value,
        unit, description, ui_widget, requires_restart, sensitivity, tags
    ) VALUES (
        p_key, 'symbol_intel', 'sizer', p_value_type, p_default,
        p_unit, p_description, 'json', FALSE, 'normal',
        ARRAY['faz14','symbol_intel','sizer']::TEXT[]
    )
    ON CONFLICT (key) DO UPDATE SET
        default_value = EXCLUDED.default_value,
        description   = EXCLUDED.description,
        updated_at    = now();
END;
$$ LANGUAGE plpgsql;

-- Tier başına max pozisyon yüzdesi (effective_risk_pct üst sınırı)
SELECT _qtss_sym_intel_seed(
    'symbol_intel.tier_cap_pct',
    'object',
    '{"core": 7.0, "balanced": 5.0, "growth": 3.5, "speculative": 2.0, "extreme": 1.0}'::JSONB,
    'pct',
    'Risk tier başına maksimum pozisyon yüzdesi (sermayenin %si).'
);

-- Regime çarpanları — risk_pct = base × regime_mul
SELECT _qtss_sym_intel_seed(
    'symbol_intel.regime_multiplier',
    'object',
    '{"risk_on": 1.0, "neutral": 0.75, "risk_off": 0.4, "panic": 0.0}'::JSONB,
    'ratio',
    'Piyasa rejimine göre risk_pct çarpanı. panic=0 → yeni pozisyon açma.'
);

-- Fundamental skor etkisi: 0..100 → 0.5..1.0 arası çarpan (lineer)
SELECT _qtss_sym_intel_seed(
    'symbol_intel.fundamental_score_range',
    'object',
    '{"floor_mul": 0.5, "ceiling_mul": 1.0}'::JSONB,
    'ratio',
    'Fundamental score 0 → floor_mul, 100 → ceiling_mul olarak risk_pct çarpanı.'
);

-- Minimum likidite tabanı (USD cinsinden 30g ort.). Altı → tier extreme'e düşer.
SELECT _qtss_sym_intel_seed(
    'symbol_intel.min_liquidity_usd',
    'object',
    '{"extreme": 100000, "speculative": 500000, "growth": 2000000, "balanced": 10000000, "core": 50000000}'::JSONB,
    'usd',
    'Tier için minimum 30-gün ort. dolar hacmi. Eşiğin altı bir alt tier''e düşer.'
);

-- Hesap sermayesi (şimdilik crypto için tek hesap, USDT bazlı default)
SELECT _qtss_sym_intel_seed(
    'symbol_intel.account_equity_seed',
    'object',
    '{"binance": 10000.0, "nasdaq": 25000.0, "bist": 1500000.0}'::JSONB,
    'quote_currency',
    'Her borsa için başlangıç sermayesi (live equity kaynağı yoksa fallback). BIST TL, diğerleri USD.'
);

DROP FUNCTION IF EXISTS _qtss_sym_intel_seed(TEXT, TEXT, JSONB, TEXT, TEXT);

COMMIT;
