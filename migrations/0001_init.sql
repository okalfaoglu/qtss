-- QTSS çekirdek şema: tek kurum (şimdilik), RBAC, config, ledger rollup, copy-trade, backtest.

CREATE TABLE organizations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE roles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key TEXT NOT NULL UNIQUE,
    description TEXT
);

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    is_admin BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE user_roles (
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);

-- Kod içinde sabit yok; kritik ayarlar burada. Admin UI: CRUD.
CREATE TABLE app_config (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key TEXT NOT NULL UNIQUE,
    value JSONB NOT NULL,
    description TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by_user_id UUID REFERENCES users (id)
);

-- Borsa API anahtarları — şimdilik düz metin; ileride vault/KMS.
CREATE TABLE exchange_accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    exchange TEXT NOT NULL,
    segment TEXT NOT NULL,
    api_key TEXT NOT NULL,
    api_secret TEXT NOT NULL,
    passphrase TEXT,
    label TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_exchange_accounts_user ON exchange_accounts (user_id);

CREATE TABLE copy_subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    leader_user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    follower_user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    rule JSONB NOT NULL,
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_copy_leader ON copy_subscriptions (leader_user_id);
CREATE INDEX idx_copy_follower ON copy_subscriptions (follower_user_id);

-- Özet P&L: job/materialized view ile doldurulacak.
CREATE TABLE pnl_rollups (
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    exchange TEXT NOT NULL,
    symbol TEXT,
    ledger TEXT NOT NULL,
    bucket TEXT NOT NULL,
    period_start TIMESTAMPTZ NOT NULL,
    realized_pnl NUMERIC NOT NULL DEFAULT 0,
    fees NUMERIC NOT NULL DEFAULT 0,
    volume NUMERIC NOT NULL DEFAULT 0,
    trade_count BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (org_id, exchange, symbol, ledger, bucket, period_start)
);

CREATE INDEX idx_pnl_ledger_bucket ON pnl_rollups (ledger, bucket, period_start DESC);

CREATE TABLE backtest_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    strategy_key TEXT NOT NULL,
    params JSONB NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    result JSONB
);

CREATE INDEX idx_backtest_org ON backtest_runs (org_id, started_at DESC);

-- Örnek roller (kurulum sonrası admin atar).
INSERT INTO roles (key, description) VALUES
    ('admin', 'Tam yetki, config ve kullanıcı yönetimi'),
    ('trader', 'Emir ve strateji'),
    ('analyst', 'Salt okunur analiz ve rapor'),
    ('viewer', 'Dashboard salt görüntüleme');
