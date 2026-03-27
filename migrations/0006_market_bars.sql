-- Normalleştirilmiş OHLCV mumları (WebSocket / REST beslemesi).

CREATE TABLE market_bars (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange TEXT NOT NULL,
    segment TEXT NOT NULL,
    symbol TEXT NOT NULL,
    interval TEXT NOT NULL,
    open_time TIMESTAMPTZ NOT NULL,
    open NUMERIC NOT NULL,
    high NUMERIC NOT NULL,
    low NUMERIC NOT NULL,
    close NUMERIC NOT NULL,
    volume NUMERIC NOT NULL,
    quote_volume NUMERIC,
    trade_count BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT market_bars_unique_bar UNIQUE (exchange, segment, symbol, interval, open_time)
);

CREATE INDEX idx_market_bars_series ON market_bars (exchange, segment, symbol, interval, open_time DESC);
