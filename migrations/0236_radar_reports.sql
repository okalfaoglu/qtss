-- QTSS RADAR periodic performance reports (Faz 20A).
--
-- One row per (market × period × mode) covering the current window.
-- The aggregator worker recomputes the "active" row of each kind on
-- each tick (the running daily / weekly / monthly / yearly report),
-- then snapshots a finalised copy when the period closes so history
-- stays queryable.
--
-- Fields mirror the QTSS RADAR screenshot template:
--   * KAPANIŞLAR      — trades array (symbol, date, notional, pnl, pct, status)
--   * SERMAYE TAKİBİ  — starting + current capital, compound return
--   * PERFORMANS      — win rate, avg allocation, avg holding
--   * RİSK METRİKLERİ — risk_mode, volatility, max position risk, cash pct

CREATE TABLE IF NOT EXISTS radar_reports (
    -- Identity.
    id              UUID        NOT NULL DEFAULT gen_random_uuid() PRIMARY KEY,
    market          TEXT        NOT NULL,          -- 'coin' | 'bist' | 'nasdaq'
    period          TEXT        NOT NULL,          -- 'daily' | 'weekly' | 'monthly' | 'yearly'
    mode            TEXT        NOT NULL,          -- 'live' | 'dry' | 'backtest'
    period_start    TIMESTAMPTZ NOT NULL,
    period_end      TIMESTAMPTZ NOT NULL,
    finalised       BOOLEAN     NOT NULL DEFAULT false,

    -- KAPANIŞLAR — serialized trade list.
    trades          JSONB       NOT NULL DEFAULT '[]'::jsonb,

    -- Aggregates.
    closed_count        INT     NOT NULL DEFAULT 0,
    win_count           INT     NOT NULL DEFAULT 0,
    loss_count          INT     NOT NULL DEFAULT 0,
    win_rate            DOUBLE PRECISION,
    total_notional_usd  DOUBLE PRECISION,
    total_pnl_usd       DOUBLE PRECISION,
    avg_return_pct      DOUBLE PRECISION,
    compound_return_pct DOUBLE PRECISION,
    avg_allocation_usd  DOUBLE PRECISION,
    avg_holding_bars    DOUBLE PRECISION,
    max_drawdown_pct    DOUBLE PRECISION,

    -- SERMAYE.
    starting_capital_usd DOUBLE PRECISION,
    ending_capital_usd   DOUBLE PRECISION,
    cash_position_pct    DOUBLE PRECISION,

    -- RİSK.
    risk_mode           TEXT,                      -- 'risk_on' | 'neutral' | 'risk_off'
    volatility_level    TEXT,                      -- 'low' | 'medium' | 'high'
    max_position_risk_pct DOUBLE PRECISION,
    correlation_risk    TEXT,

    computed_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (market, period, mode, period_start)
);

CREATE INDEX IF NOT EXISTS radar_reports_recent_idx
    ON radar_reports (market, period, mode, period_end DESC);

CREATE INDEX IF NOT EXISTS radar_reports_active_idx
    ON radar_reports (market, period, mode)
    WHERE finalised = false;

COMMENT ON TABLE radar_reports IS
    'Periodic performance aggregates — one row per (market × period × mode). Active (non-finalised) row refreshed on each aggregator tick; finalised at period end. Drives the /v2/reports GUI page and future PDF export.';

-- Config seeds for the aggregator.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('radar', 'enabled', '{"enabled": true}'::jsonb,
     'Master on/off for the RADAR reports aggregator loop.'),
    ('radar', 'tick_secs', '{"secs": 300}'::jsonb,
     'Aggregator cadence (seconds). 300s = 5 minutes keeps the active daily/weekly row fresh without hammering the DB.'),
    ('radar', 'default_starting_capital_usd', '{"value": 1500000}'::jsonb,
     'Varsayılan başlangıç sermayesi ($). Dry modda matematiksel; live için her kullanıcının account equity''si üzerine yazılır.'),
    ('radar', 'risk_mode.risk_on_win_rate', '{"value": 0.65}'::jsonb,
     'Bu win_rate üstünde risk_mode=risk_on. Altında nötr, 0.4 altında risk_off.'),
    ('radar', 'risk_mode.risk_off_win_rate', '{"value": 0.40}'::jsonb,
     'Bu win_rate altında risk_mode=risk_off.'),
    ('radar', 'volatility.high_std_pct', '{"value": 0.05}'::jsonb,
     'Getiri std sapması bu oranın üstündeyse volatilite=high.'),
    ('radar', 'volatility.medium_std_pct', '{"value": 0.02}'::jsonb,
     'Getiri std sapması bu oranın üstündeyse volatilite=medium, altındaysa low.')
ON CONFLICT (module, config_key) DO NOTHING;
