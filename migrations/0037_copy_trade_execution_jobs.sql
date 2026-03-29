-- Copy trade: leader fill → follower execution queue (dev guide §9.1 item 4).

CREATE TABLE copy_trade_execution_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subscription_id UUID NOT NULL REFERENCES copy_subscriptions (id) ON DELETE CASCADE,
    leader_exchange_order_id UUID NOT NULL REFERENCES exchange_orders (id) ON DELETE CASCADE,
    follower_user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    leader_user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT copy_trade_execution_jobs_sub_leader UNIQUE (subscription_id, leader_exchange_order_id)
);

CREATE INDEX idx_copy_trade_jobs_pending_created ON copy_trade_execution_jobs (created_at ASC)
WHERE
    status = 'pending';
