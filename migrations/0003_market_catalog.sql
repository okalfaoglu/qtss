-- Borsa (venue), piyasa (spot/futures/…), enstrüman (sembol) kataloğu — connector senkronundan doldurulur.

CREATE TABLE exchanges (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT true,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE markets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange_id UUID NOT NULL REFERENCES exchanges (id) ON DELETE CASCADE,
    segment TEXT NOT NULL,
    contract_kind TEXT NOT NULL DEFAULT '',
    display_name TEXT,
    is_active BOOLEAN NOT NULL DEFAULT true,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT markets_segment_check CHECK (
        segment IN ('spot', 'futures', 'margin', 'options')
    ),
    UNIQUE (exchange_id, segment, contract_kind)
);

CREATE INDEX idx_markets_exchange ON markets (exchange_id);

CREATE TABLE instruments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    market_id UUID NOT NULL REFERENCES markets (id) ON DELETE CASCADE,
    native_symbol TEXT NOT NULL,
    base_asset TEXT NOT NULL,
    quote_asset TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'unknown',
    is_trading BOOLEAN NOT NULL DEFAULT false,
    price_filter JSONB,
    lot_filter JSONB,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (market_id, native_symbol)
);

CREATE INDEX idx_instruments_market ON instruments (market_id);
CREATE INDEX idx_instruments_base_quote ON instruments (base_asset, quote_asset);
CREATE INDEX idx_instruments_native_symbol ON instruments (native_symbol);
CREATE INDEX idx_instruments_trading ON instruments (market_id) WHERE is_trading = true;

INSERT INTO exchanges (code, display_name)
VALUES ('binance', 'Binance')
ON CONFLICT (code) DO NOTHING;
