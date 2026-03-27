-- Borsaya iletilen emirler — denetim, idempotency ve ileride mutabakat için.

CREATE TABLE exchange_orders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    exchange TEXT NOT NULL,
    segment TEXT NOT NULL,
    symbol TEXT NOT NULL,
    client_order_id UUID NOT NULL,
    status TEXT NOT NULL DEFAULT 'submitted',
    intent JSONB NOT NULL,
    venue_order_id BIGINT,
    venue_response JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT exchange_orders_user_client UNIQUE (user_id, client_order_id)
);

CREATE INDEX idx_exchange_orders_user_created ON exchange_orders (user_id, created_at DESC);
CREATE INDEX idx_exchange_orders_org_created ON exchange_orders (org_id, created_at DESC);
