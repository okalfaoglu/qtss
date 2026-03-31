-- Binance user stream / reconcile kaynaklı dolumlar (fill) — emir kapanışı ve raporlama için.

CREATE TABLE exchange_fills (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    exchange TEXT NOT NULL,
    segment TEXT NOT NULL,
    symbol TEXT NOT NULL,
    venue_order_id BIGINT NOT NULL,
    venue_trade_id BIGINT,
    fill_price NUMERIC,
    fill_quantity NUMERIC,
    fee NUMERIC,
    fee_asset TEXT,
    event_time TIMESTAMPTZ NOT NULL DEFAULT now(),
    raw_event JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT exchange_fills_unique_trade UNIQUE (exchange, segment, user_id, venue_order_id, venue_trade_id)
);

CREATE INDEX idx_exchange_fills_user_time ON exchange_fills (user_id, event_time DESC);
CREATE INDEX idx_exchange_fills_order ON exchange_fills (exchange, segment, user_id, venue_order_id);

