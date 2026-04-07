-- QTSS baseline: single migration squashed from historical NNNN_*.sql chain.
-- Fresh databases only (or drop _sqlx_migrations / full DB reset).
-- Regenerate: python3 scripts/squash_migrations_into_one.py

-- >>> merged from: 0001_init.sql
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

-- >>> merged from: 0002_oauth.sql
-- OAuth 2.0 tarzı istemciler ve yenileme belirteçleri (RFC 6749).

CREATE TABLE oauth_clients (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    client_id TEXT NOT NULL UNIQUE,
    client_secret_hash TEXT NOT NULL,
    allowed_grant_types TEXT[] NOT NULL,
    service_user_id UUID REFERENCES users (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_oauth_clients_org ON oauth_clients (org_id);

COMMENT ON COLUMN oauth_clients.service_user_id IS 'client_credentials için bu kullanıcı adına JWT üretilir.';

CREATE TABLE refresh_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_uuid UUID NOT NULL REFERENCES oauth_clients (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_refresh_user ON refresh_tokens (user_id);
CREATE INDEX idx_refresh_expires ON refresh_tokens (expires_at) WHERE revoked_at IS NULL;

-- >>> merged from: 0003_market_catalog.sql
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

-- >>> merged from: 0004_exchange_orders.sql
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

-- >>> merged from: 0005_audit_log.sql
-- HTTP mutasyonları için denetim izi (isteğe bağlı tetik; bakınız QTSS_AUDIT_HTTP).

CREATE TABLE audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    request_id TEXT,
    user_id UUID REFERENCES users (id) ON DELETE SET NULL,
    org_id UUID REFERENCES organizations (id) ON DELETE SET NULL,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    status_code SMALLINT NOT NULL,
    roles TEXT[] NOT NULL DEFAULT '{}'
);

CREATE INDEX idx_audit_log_created ON audit_log (created_at DESC);
CREATE INDEX idx_audit_log_user ON audit_log (user_id, created_at DESC);

-- >>> merged from: 0006_market_bars.sql
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

-- >>> merged from: 0007_acp_chart_patterns.sql
-- Pine: Auto Chart Patterns [Trendoscope®] v6 — useZigzag1..4 (yalnız z1 açık), lastPivot both, ScanProperties.offset=0.

INSERT INTO app_config (key, value, description)
VALUES (
    'acp_chart_patterns',
    '{
      "version": 1,
      "ohlc": { "open": "open", "high": "high", "low": "low", "close": "close" },
      "zigzag": [
        { "enabled": true, "length": 8, "depth": 55 },
        { "enabled": false, "length": 13, "depth": 34 },
        { "enabled": false, "length": 21, "depth": 21 },
        { "enabled": false, "length": 34, "depth": 13 }
      ],
      "scanning": {
        "number_of_pivots": 5,
        "error_threshold_percent": 20,
        "flat_threshold_percent": 20,
        "verify_bar_ratio": true,
        "bar_ratio_limit": 0.382,
        "avoid_overlap": true,
        "repaint": false,
        "pivot_tail_skip_max": 0,
        "max_zigzag_levels": 0,
        "upper_direction": 1,
        "lower_direction": -1,
        "ignore_if_entry_crossed": false,
        "size_filters": {
          "filter_by_bar": false,
          "min_pattern_bars": 0,
          "max_pattern_bars": 1000,
          "filter_by_percent": false,
          "min_pattern_percent": 0,
          "max_pattern_percent": 100
        }
      },
      "patterns": {
        "1": { "enabled": true, "last_pivot": "both" },
        "2": { "enabled": true, "last_pivot": "both" },
        "3": { "enabled": true, "last_pivot": "both" },
        "4": { "enabled": true, "last_pivot": "both" },
        "5": { "enabled": true, "last_pivot": "both" },
        "6": { "enabled": true, "last_pivot": "both" },
        "7": { "enabled": true, "last_pivot": "both" },
        "8": { "enabled": true, "last_pivot": "both" },
        "9": { "enabled": true, "last_pivot": "both" },
        "10": { "enabled": true, "last_pivot": "both" },
        "11": { "enabled": true, "last_pivot": "both" },
        "12": { "enabled": true, "last_pivot": "both" },
        "13": { "enabled": true, "last_pivot": "both" }
      },
      "display": {
        "theme": "dark",
        "pattern_line_width": 2,
        "zigzag_line_width": 1,
        "show_pattern_label": true,
        "show_pivot_labels": true,
        "show_zigzag": true,
        "max_patterns": 20
      },
      "calculated_bars": 5000
    }'::jsonb,
    'ACP [Trendoscope] — TV göstergesi ile hizalı zigzag / tarama / desen filtreleri (GUI + kanal taraması).'
)
ON CONFLICT (key) DO NOTHING;

-- >>> merged from: 0008_acp_zigzag_seven_fib.sql
-- Pine ACP v6 fabrika: 4 zigzag (useZigzag1 açık), pivot_tail_skip_max=0, last_pivot both (lastPivotDirection=both).
-- Not: Dosya adı tarihsel (önceki sürüm 7 Fib idi); yeni kurulumlarda 0007 ile aynı hedef.

UPDATE app_config
SET value = jsonb_set(
  jsonb_set(
    jsonb_set(
      value,
      '{zigzag}',
      '[
        {"enabled": true, "length": 8, "depth": 55},
        {"enabled": false, "length": 13, "depth": 34},
        {"enabled": false, "length": 21, "depth": 21},
        {"enabled": false, "length": 34, "depth": 13}
      ]'::jsonb
    ),
    '{scanning}',
    coalesce(value->'scanning', '{}'::jsonb) || '{"pivot_tail_skip_max": 0}'::jsonb
  ),
  '{patterns}',
  '{
    "1": {"enabled": true, "last_pivot": "both"},
    "2": {"enabled": true, "last_pivot": "both"},
    "3": {"enabled": true, "last_pivot": "both"},
    "4": {"enabled": true, "last_pivot": "both"},
    "5": {"enabled": true, "last_pivot": "both"},
    "6": {"enabled": true, "last_pivot": "both"},
    "7": {"enabled": true, "last_pivot": "both"},
    "8": {"enabled": true, "last_pivot": "both"},
    "9": {"enabled": true, "last_pivot": "both"},
    "10": {"enabled": true, "last_pivot": "both"},
    "11": {"enabled": true, "last_pivot": "both"},
    "12": {"enabled": true, "last_pivot": "both"},
    "13": {"enabled": true, "last_pivot": "both"}
  }'::jsonb
)
WHERE key = 'acp_chart_patterns';

-- >>> merged from: 0009_acp_pine_indicator_defaults.sql
-- Eski 0008 (7 Fib) uygulanmış DB’ler veya elle kırılmış zigzag/pivot ayarları — TV ACP v6 fabrika ile hizala.

UPDATE app_config
SET value = jsonb_set(
  jsonb_set(
    jsonb_set(
      value,
      '{zigzag}',
      '[
        {"enabled": true, "length": 8, "depth": 55},
        {"enabled": false, "length": 13, "depth": 34},
        {"enabled": false, "length": 21, "depth": 21},
        {"enabled": false, "length": 34, "depth": 13}
      ]'::jsonb
    ),
    '{scanning}',
    coalesce(value->'scanning', '{}'::jsonb) || '{"pivot_tail_skip_max": 0}'::jsonb
  ),
  '{patterns}',
  '{
    "1": {"enabled": true, "last_pivot": "both"},
    "2": {"enabled": true, "last_pivot": "both"},
    "3": {"enabled": true, "last_pivot": "both"},
    "4": {"enabled": true, "last_pivot": "both"},
    "5": {"enabled": true, "last_pivot": "both"},
    "6": {"enabled": true, "last_pivot": "both"},
    "7": {"enabled": true, "last_pivot": "both"},
    "8": {"enabled": true, "last_pivot": "both"},
    "9": {"enabled": true, "last_pivot": "both"},
    "10": {"enabled": true, "last_pivot": "both"},
    "11": {"enabled": true, "last_pivot": "both"},
    "12": {"enabled": true, "last_pivot": "both"},
    "13": {"enabled": true, "last_pivot": "both"}
  }'::jsonb
)
WHERE key = 'acp_chart_patterns';

-- >>> merged from: 0010_acp_abstract_size_filters.sql
-- Pine abstractchartpatterns: ScanProperties.ignoreIfEntryCrossed + SizeFilters (yalnız eksikse eklenir).

UPDATE app_config
SET value = jsonb_set(
  value,
  '{scanning}',
  coalesce(value->'scanning', '{}'::jsonb)
    || CASE
      WHEN (value->'scanning' ? 'ignore_if_entry_crossed') THEN '{}'::jsonb
      ELSE '{"ignore_if_entry_crossed": false}'::jsonb
    END
    || CASE
      WHEN (value->'scanning' ? 'size_filters') THEN '{}'::jsonb
      ELSE '{
        "size_filters": {
          "filter_by_bar": false,
          "min_pattern_bars": 0,
          "max_pattern_bars": 1000,
          "filter_by_percent": false,
          "min_pattern_percent": 0,
          "max_pattern_percent": 100
        }
      }'::jsonb
    END
)
WHERE key = 'acp_chart_patterns';

-- >>> merged from: 0011_acp_last_pivot_direction.sql
-- Pine `lastPivotDirection` karşılığı: varsayılan `both` (allowedLastPivotDirections = hepsi serbest).

UPDATE app_config
SET value = jsonb_set(
  coalesce(value, '{}'::jsonb),
  '{scanning}',
  coalesce(value->'scanning', '{}'::jsonb) || '{"last_pivot_direction": "both"}'::jsonb
)
WHERE key = 'acp_chart_patterns'
  AND (value->'scanning'->>'last_pivot_direction' IS NULL);

-- >>> merged from: 0012_acp_pattern_groups.sql
-- Pine `allowedPatterns` grupları (geometri / yön / dinamik); eksikse tümü açık varsayılır.

UPDATE app_config
SET value = coalesce(value, '{}'::jsonb) || '{
  "pattern_groups": {
    "geometric": { "channels": true, "wedges": true, "triangles": true },
    "direction": { "rising": true, "falling": true, "flat_bidirectional": true },
    "formation_dynamics": { "expanding": true, "contracting": true, "parallel": true }
  }
}'::jsonb
WHERE key = 'acp_chart_patterns'
  AND NOT (coalesce(value, '{}'::jsonb) ? 'pattern_groups');

-- >>> merged from: 0013_worker_analytics_schema.sql
-- Worker / analiz çekirdeği: `qtss-storage` + `qtss-worker` ile uyumlu tablolar.
-- Yeni kurulum: 0001–0012 sonrası. Mevcut DB: IF NOT EXISTS / ADD COLUMN IF NOT EXISTS.
-- Not: Eski ayrı `0013_bar_intervals.sql` ile çakışmayı önlemek için bar_intervals genişletmesi burada birleştirildi (tek SQLx sürümü 13).

ALTER TABLE markets DROP CONSTRAINT IF EXISTS markets_segment_check;

ALTER TABLE market_bars
    ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS bar_interval_id UUID;

CREATE TABLE IF NOT EXISTS bar_intervals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL,
    label TEXT,
    duration_seconds INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT bar_intervals_code_key UNIQUE (code)
);

DO $$
BEGIN
    ALTER TABLE market_bars
        ADD CONSTRAINT market_bars_bar_interval_id_fkey FOREIGN KEY (bar_interval_id) REFERENCES bar_intervals (id) ON DELETE SET NULL;
EXCEPTION
    WHEN duplicate_object THEN NULL;
END
$$;

INSERT INTO bar_intervals (code, label, duration_seconds, sort_order, is_active, metadata)
VALUES
    ('1m', '1 minute', 60, 10, TRUE, '{}'),
    ('3m', '3 minutes', 180, 20, TRUE, '{}'),
    ('5m', '5 minutes', 300, 30, TRUE, '{}'),
    ('15m', '15 minutes', 900, 40, TRUE, '{}'),
    ('30m', '30 minutes', 1800, 50, TRUE, '{}'),
    ('1h', '1 hour', 3600, 60, TRUE, '{}'),
    ('2h', '2 hours', 7200, 70, TRUE, '{}'),
    ('4h', '4 hours', 14400, 80, TRUE, '{}'),
    ('1d', '1 day', 86400, 90, TRUE, '{}')
ON CONFLICT (code) DO NOTHING;

INSERT INTO bar_intervals (code, label, duration_seconds, sort_order, is_active, metadata) VALUES
    ('1s', '1 saniye', 1, 5, TRUE, '{}'),
    ('6h', '6 saat', 21600, 65, TRUE, '{}'),
    ('8h', '8 saat', 28800, 70, TRUE, '{}'),
    ('12h', '12 saat', 43200, 75, TRUE, '{}'),
    ('3d', '3 gün', 259200, 85, TRUE, '{}'),
    ('1w', '1 hafta', 604800, 90, TRUE, '{}'),
    ('1M', '1 ay', NULL, 100, TRUE, '{}')
ON CONFLICT (code) DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_bar_intervals_active ON bar_intervals (is_active, sort_order);

COMMENT ON TABLE bar_intervals IS 'OHLC mum aralığı kataloğu; market_bars / engine_symbols FK ile tekrarlayan metin azaltılır.';

CREATE TABLE IF NOT EXISTS engine_symbols (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange TEXT NOT NULL,
    segment TEXT NOT NULL,
    symbol TEXT NOT NULL,
    interval TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    label TEXT,
    signal_direction_mode TEXT NOT NULL DEFAULT 'auto_segment',
    exchange_id UUID REFERENCES exchanges (id) ON DELETE SET NULL,
    market_id UUID REFERENCES markets (id) ON DELETE SET NULL,
    instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
    bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT engine_symbols_series_key UNIQUE (exchange, segment, symbol, interval)
);

ALTER TABLE engine_symbols
    ADD COLUMN IF NOT EXISTS exchange_id UUID REFERENCES exchanges (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS market_id UUID REFERENCES markets (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS signal_direction_mode TEXT;

UPDATE engine_symbols
SET signal_direction_mode = 'auto_segment'
WHERE signal_direction_mode IS NULL;

ALTER TABLE engine_symbols
    ALTER COLUMN signal_direction_mode SET DEFAULT 'auto_segment';

ALTER TABLE engine_symbols
    ALTER COLUMN signal_direction_mode SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_engine_symbols_enabled ON engine_symbols (enabled)
WHERE
    enabled = TRUE;

CREATE TABLE IF NOT EXISTS analysis_snapshots (
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    engine_kind TEXT NOT NULL,
    payload JSONB NOT NULL,
    last_bar_open_time TIMESTAMPTZ,
    bar_count INTEGER,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT,
    PRIMARY KEY (engine_symbol_id, engine_kind)
);

CREATE TABLE IF NOT EXISTS range_signal_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    event_kind TEXT NOT NULL,
    bar_open_time TIMESTAMPTZ NOT NULL,
    reference_price DOUBLE PRECISION,
    source TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT range_signal_events_dedup UNIQUE (engine_symbol_id, event_kind, bar_open_time)
);

CREATE INDEX IF NOT EXISTS idx_range_signal_events_engine_time ON range_signal_events (engine_symbol_id, bar_open_time DESC);

CREATE TABLE IF NOT EXISTS data_snapshots (
    source_key TEXT PRIMARY KEY,
    request_json JSONB NOT NULL,
    response_json JSONB,
    meta_json JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);

CREATE TABLE IF NOT EXISTS nansen_snapshots (
    snapshot_kind TEXT PRIMARY KEY,
    request_json JSONB NOT NULL,
    response_json JSONB,
    meta_json JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);

CREATE TABLE IF NOT EXISTS external_data_sources (
    key TEXT PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    headers_json JSONB NOT NULL DEFAULT '{}',
    body_json JSONB,
    tick_secs INTEGER NOT NULL DEFAULT 30,
    description TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS onchain_signal_scores (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol TEXT NOT NULL,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    funding_score DOUBLE PRECISION,
    oi_score DOUBLE PRECISION,
    ls_ratio_score DOUBLE PRECISION,
    taker_vol_score DOUBLE PRECISION,
    exchange_netflow_score DOUBLE PRECISION,
    exchange_balance_score DOUBLE PRECISION,
    hl_bias_score DOUBLE PRECISION,
    hl_whale_score DOUBLE PRECISION,
    liquidation_score DOUBLE PRECISION,
    nansen_sm_score DOUBLE PRECISION,
    nansen_netflow_score DOUBLE PRECISION,
    nansen_perp_score DOUBLE PRECISION,
    nansen_buyer_quality_score DOUBLE PRECISION,
    tvl_trend_score DOUBLE PRECISION,
    aggregate_score DOUBLE PRECISION NOT NULL,
    confidence DOUBLE PRECISION NOT NULL,
    direction TEXT NOT NULL,
    market_regime TEXT,
    conflict_detected BOOLEAN NOT NULL DEFAULT FALSE,
    conflict_detail TEXT,
    snapshot_keys TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    meta_json JSONB
);

CREATE INDEX IF NOT EXISTS idx_onchain_signal_scores_symbol_time ON onchain_signal_scores (symbol, computed_at DESC);

CREATE TABLE IF NOT EXISTS market_confluence_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    schema_version INTEGER NOT NULL,
    regime TEXT,
    composite_score DOUBLE PRECISION NOT NULL,
    confidence_0_100 INTEGER NOT NULL,
    scores_json JSONB NOT NULL,
    conflicts_json JSONB NOT NULL,
    confluence_payload_json JSONB
);

CREATE INDEX IF NOT EXISTS idx_market_confluence_engine_time ON market_confluence_snapshots (engine_symbol_id, computed_at DESC);

CREATE TABLE IF NOT EXISTS nansen_setup_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    request_json JSONB NOT NULL,
    source TEXT NOT NULL,
    candidate_count INTEGER NOT NULL,
    meta_json JSONB,
    error TEXT
);

CREATE INDEX IF NOT EXISTS idx_nansen_setup_runs_computed ON nansen_setup_runs (computed_at DESC);

CREATE TABLE IF NOT EXISTS nansen_setup_rows (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES nansen_setup_runs (id) ON DELETE CASCADE,
    rank INTEGER NOT NULL,
    chain TEXT NOT NULL,
    token_address TEXT NOT NULL,
    token_symbol TEXT NOT NULL,
    direction TEXT NOT NULL,
    score INTEGER NOT NULL,
    probability DOUBLE PRECISION NOT NULL,
    setup TEXT NOT NULL,
    key_signals JSONB NOT NULL,
    entry DOUBLE PRECISION NOT NULL,
    stop_loss DOUBLE PRECISION NOT NULL,
    tp1 DOUBLE PRECISION NOT NULL,
    tp2 DOUBLE PRECISION NOT NULL,
    tp3 DOUBLE PRECISION NOT NULL,
    rr DOUBLE PRECISION NOT NULL,
    pct_to_tp2 DOUBLE PRECISION NOT NULL,
    ohlc_enriched BOOLEAN NOT NULL DEFAULT FALSE,
    raw_metrics JSONB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_nansen_setup_rows_run ON nansen_setup_rows (run_id, rank);

CREATE TABLE IF NOT EXISTS paper_balances (
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    quote_balance NUMERIC NOT NULL,
    base_positions JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id)
);

CREATE TABLE IF NOT EXISTS paper_fills (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    exchange TEXT NOT NULL,
    segment TEXT NOT NULL,
    symbol TEXT NOT NULL,
    client_order_id UUID NOT NULL,
    side TEXT NOT NULL,
    quantity NUMERIC NOT NULL,
    avg_price NUMERIC NOT NULL,
    fee NUMERIC NOT NULL,
    quote_balance_after NUMERIC NOT NULL,
    base_positions_after JSONB NOT NULL,
    intent JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_paper_fills_user_created ON paper_fills (user_id, created_at DESC);

-- >>> merged from: 0014_catalog_fk_columns.sql
-- engine_symbols / market_bars → katalog FK (metin kolonları geriye dönük uyumluluk için kalır).

DO $$
BEGIN
  IF to_regclass('public.engine_symbols') IS NOT NULL THEN
    ALTER TABLE engine_symbols
      ADD COLUMN IF NOT EXISTS exchange_id UUID REFERENCES exchanges (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS market_id UUID REFERENCES markets (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL;
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_exchange_id ON engine_symbols (exchange_id);
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_market_id ON engine_symbols (market_id);
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_instrument_id ON engine_symbols (instrument_id);
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_bar_interval_id ON engine_symbols (bar_interval_id);
  END IF;

  IF to_regclass('public.market_bars') IS NOT NULL THEN
    ALTER TABLE market_bars
      ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL;
    CREATE INDEX IF NOT EXISTS idx_market_bars_instrument_interval_time
      ON market_bars (instrument_id, bar_interval_id, open_time DESC)
      WHERE instrument_id IS NOT NULL AND bar_interval_id IS NOT NULL;
  END IF;
END $$;

-- bar_interval_id doldur (metin interval ile)
UPDATE market_bars mb
SET bar_interval_id = bi.id
FROM bar_intervals bi
WHERE mb.bar_interval_id IS NULL
  AND LOWER(TRIM(mb.interval)) = LOWER(bi.code);

-- instrument_id: borsa + segment eşlemesi (worker segment: spot | futures)
UPDATE market_bars mb
SET instrument_id = i.id
FROM instruments i
INNER JOIN markets m ON m.id = i.market_id
INNER JOIN exchanges e ON e.id = m.exchange_id
WHERE mb.instrument_id IS NULL
  AND LOWER(TRIM(mb.exchange)) = LOWER(e.code)
  AND (
    (LOWER(TRIM(mb.segment)) = 'spot' AND m.segment = 'spot' AND (m.contract_kind = '' OR m.contract_kind IS NULL))
    OR (
      LOWER(TRIM(mb.segment)) IN ('futures', 'usdt_futures', 'fapi')
      AND m.segment = 'futures'
      AND m.contract_kind = 'usdt_m'
    )
  )
  AND UPPER(TRIM(mb.symbol)) = UPPER(i.native_symbol);

UPDATE engine_symbols es
SET bar_interval_id = bi.id
FROM bar_intervals bi
WHERE es.bar_interval_id IS NULL
  AND LOWER(TRIM(es.interval)) = LOWER(bi.code);

UPDATE engine_symbols es
SET exchange_id = e.id
FROM exchanges e
WHERE es.exchange_id IS NULL
  AND LOWER(TRIM(es.exchange)) = LOWER(e.code);

UPDATE engine_symbols es
SET market_id = m.id
FROM markets m
INNER JOIN exchanges e ON e.id = m.exchange_id
WHERE es.market_id IS NULL
  AND es.exchange_id = e.id
  AND (
    (LOWER(TRIM(es.segment)) = 'spot' AND m.segment = 'spot' AND (m.contract_kind = '' OR m.contract_kind IS NULL))
    OR (
      LOWER(TRIM(es.segment)) IN ('futures', 'usdt_futures', 'fapi')
      AND m.segment = 'futures'
      AND m.contract_kind = 'usdt_m'
    )
  );

UPDATE engine_symbols es
SET instrument_id = i.id
FROM instruments i
WHERE es.instrument_id IS NULL
  AND es.market_id = i.market_id
  AND UPPER(TRIM(es.symbol)) = UPPER(i.native_symbol);

-- ACP: üst çubuk TF değişince otomatik kanal taraması (varsayılan kapalı).

UPDATE app_config
SET value = jsonb_set(
    value,
    '{scanning}',
    coalesce(value->'scanning', '{}'::jsonb)
      || '{"auto_scan_on_timeframe_change": false}'::jsonb,
    true
  ),
  updated_at = now()
WHERE key = 'acp_chart_patterns'
  AND (value->'scanning'->'auto_scan_on_timeframe_change' IS NULL);

-- >>> merged from: 0015_engine_analysis.sql
-- Arka plan motor hedefleri + analiz snapshot’ları (`qtss-worker` engine_analysis, confluence).
-- `0013_worker_analytics_schema.sql` zaten `engine_symbols` / `analysis_snapshots` oluşturabildiği için IF NOT EXISTS.

CREATE TABLE IF NOT EXISTS engine_symbols (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange TEXT NOT NULL DEFAULT 'binance',
    segment TEXT NOT NULL DEFAULT 'spot',
    symbol TEXT NOT NULL,
    interval TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    sort_order INT NOT NULL DEFAULT 0,
    label TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (exchange, segment, symbol, interval)
);

CREATE INDEX IF NOT EXISTS idx_engine_symbols_symbol ON engine_symbols (symbol);
CREATE INDEX IF NOT EXISTS idx_engine_symbols_enabled ON engine_symbols (enabled) WHERE enabled = true;

CREATE TABLE IF NOT EXISTS analysis_snapshots (
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    engine_kind TEXT NOT NULL,
    payload JSONB NOT NULL,
    last_bar_open_time TIMESTAMPTZ,
    bar_count INT,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT,
    PRIMARY KEY (engine_symbol_id, engine_kind)
);

CREATE INDEX IF NOT EXISTS idx_analysis_snapshots_kind ON analysis_snapshots (engine_kind);

-- >>> merged from: 0016_range_signal_events.sql
-- Range sinyal olayları (F1): worker `insert_range_signal_event` — aynı (hedef, tür, bar) tekrar yazılmaz.
-- `0013_worker_analytics_schema.sql` bu tabloyu da oluşturabildiği için IF NOT EXISTS.

CREATE TABLE IF NOT EXISTS range_signal_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    event_kind TEXT NOT NULL,
    bar_open_time TIMESTAMPTZ NOT NULL,
    reference_price DOUBLE PRECISION,
    source TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (engine_symbol_id, event_kind, bar_open_time)
);

CREATE INDEX IF NOT EXISTS idx_range_signal_events_engine_time
  ON range_signal_events (engine_symbol_id, bar_open_time DESC);

-- >>> merged from: 0017_paper_ledger.sql
-- Dry / paper ledger (`PaperLedgerRepository`).
-- Idempotent: `0013_worker_analytics_schema.sql` may already create these tables.

CREATE TABLE IF NOT EXISTS paper_balances (
    user_id UUID NOT NULL PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    quote_balance NUMERIC NOT NULL,
    base_positions JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS paper_fills (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    exchange TEXT NOT NULL,
    segment TEXT NOT NULL,
    symbol TEXT NOT NULL,
    client_order_id UUID NOT NULL,
    side TEXT NOT NULL,
    quantity NUMERIC NOT NULL,
    avg_price NUMERIC NOT NULL,
    fee NUMERIC NOT NULL,
    quote_balance_after NUMERIC NOT NULL,
    base_positions_after JSONB NOT NULL,
    intent JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_paper_fills_user_created ON paper_fills (user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_paper_fills_created ON paper_fills (created_at);

-- >>> merged from: 0018_engine_signal_direction_mode.sql
-- Motor hedefi: `both` | `long_only` | `short_only` | `auto_segment` (`update_engine_symbol_patch`, worker politika).

ALTER TABLE engine_symbols
  ADD COLUMN IF NOT EXISTS signal_direction_mode TEXT NOT NULL DEFAULT 'auto_segment';

-- >>> merged from: 0019_nansen_snapshots.sql
-- Nansen token screener: worker `upsert_nansen_snapshot` — `snapshot_kind` başına tek satır.
-- Idempotent: `0013_worker_analytics_schema.sql` may already create this table.

CREATE TABLE IF NOT EXISTS nansen_snapshots (
    snapshot_kind TEXT PRIMARY KEY,
    request_json JSONB NOT NULL,
    response_json JSONB,
    meta_json JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);

-- >>> merged from: 0020_nansen_setup_scans.sql
-- Setup taraması: `setup_scan_engine` → `insert_nansen_setup_run` / `insert_nansen_setup_row`.
-- Idempotent: `0013_worker_analytics_schema.sql` may already create these tables.

CREATE TABLE IF NOT EXISTS nansen_setup_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    request_json JSONB NOT NULL,
    source TEXT NOT NULL,
    candidate_count INT NOT NULL,
    meta_json JSONB,
    error TEXT
);

CREATE INDEX IF NOT EXISTS idx_nansen_setup_runs_computed ON nansen_setup_runs (computed_at DESC);

CREATE TABLE IF NOT EXISTS nansen_setup_rows (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES nansen_setup_runs (id) ON DELETE CASCADE,
    rank INT NOT NULL,
    chain TEXT NOT NULL,
    token_address TEXT NOT NULL,
    token_symbol TEXT NOT NULL,
    direction TEXT NOT NULL,
    score INT NOT NULL,
    probability DOUBLE PRECISION NOT NULL,
    setup TEXT NOT NULL,
    key_signals JSONB NOT NULL,
    entry DOUBLE PRECISION NOT NULL,
    stop_loss DOUBLE PRECISION NOT NULL,
    tp1 DOUBLE PRECISION NOT NULL,
    tp2 DOUBLE PRECISION NOT NULL,
    tp3 DOUBLE PRECISION NOT NULL,
    rr DOUBLE PRECISION NOT NULL,
    pct_to_tp2 DOUBLE PRECISION NOT NULL,
    ohlc_enriched BOOLEAN NOT NULL DEFAULT false,
    raw_metrics JSONB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_nansen_setup_rows_run ON nansen_setup_rows (run_id, rank);

-- >>> merged from: 0021_external_data_fetch.sql
-- Harici HTTP kaynak tanımları; yanıtlar `data_snapshots` (`external_fetch_engine`, ops API).
-- Idempotent: `0013_worker_analytics_schema.sql` may already create `external_data_sources`.
-- Ensures `created_at` and the partial index from the original 0021 migration.

CREATE TABLE IF NOT EXISTS external_data_sources (
    key TEXT PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    headers_json JSONB NOT NULL DEFAULT '{}',
    body_json JSONB,
    tick_secs INTEGER NOT NULL DEFAULT 30,
    description TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE external_data_sources
    ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT now();

CREATE INDEX IF NOT EXISTS idx_external_data_sources_enabled ON external_data_sources (enabled) WHERE enabled = true;

-- >>> merged from: 0022_data_snapshots_confluence.sql
-- Birleşik anlık görüntü: `source_key` başına tek satır (Nansen + generic HTTP + confluence okuma).
-- Idempotent: `0013_worker_analytics_schema.sql` may already create `data_snapshots`.

CREATE TABLE IF NOT EXISTS data_snapshots (
    source_key TEXT PRIMARY KEY,
    request_json JSONB NOT NULL,
    response_json JSONB,
    meta_json JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);

CREATE INDEX IF NOT EXISTS idx_data_snapshots_computed ON data_snapshots (computed_at DESC);

-- >>> merged from: 0023_external_data_sources_seed_f7.sql
-- F7 örnek kaynaklar (Coinglass anahtarı / URL’ler ortamınıza göre güncellenmeli). `ON CONFLICT` ile idempotent.

INSERT INTO external_data_sources (key, enabled, method, url, headers_json, body_json, tick_secs, description)
VALUES
(
    'coinglass_netflow_btc',
    false,
    'GET',
    'https://open-api.coinglass.com/public/v2/exchange/netflow?symbol=BTC&ex=Binance',
    '{}'::jsonb,
    NULL,
    300,
    'BTC Binance netflow proxy — Coinglass API key: headers_json (PLAN §2).'
),
(
    'binance_taker_btcusdt',
    true,
    'GET',
    'https://fapi.binance.com/futures/data/takerlongshortRatio?symbol=BTCUSDT&period=5m&limit=10',
    '{}'::jsonb,
    NULL,
    60,
    'BTCUSDT taker long/short ratio (public FAPI).'
),
(
    'coinglass_exchange_balance_btc',
    false,
    'GET',
    'https://open-api.coinglass.com/public/v2/exchange/balance?symbol=BTC',
    '{}'::jsonb,
    NULL,
    300,
    'BTC multi-exchange balance proxy — Coinglass key gerekir.'
)
ON CONFLICT (key) DO NOTHING;

-- >>> merged from: 0024_drop_external_data_snapshots.sql
-- Eski tablo: ham HTTP yanıtları artık yalnızca `data_snapshots` içinde.

DROP TABLE IF EXISTS external_data_snapshots;

-- >>> merged from: 0025_confluence_weights_app_config.sql
-- PLAN §4.1 — default regime weights (English keys). Admin may override via `PUT /api/v1/config` key `confluence_weights_by_regime`.

INSERT INTO app_config (key, value, description)
VALUES (
    'confluence_weights_by_regime',
    '{
      "range": { "technical": 0.50, "onchain": 0.35, "smart_money": 0.15 },
      "trend": { "technical": 0.30, "onchain": 0.40, "smart_money": 0.30 },
      "breakout": { "technical": 0.40, "onchain": 0.45, "smart_money": 0.15 },
      "uncertain": { "technical": 0.20, "onchain": 0.30, "smart_money": 0.50 }
    }'::jsonb,
    'Worker confluence: pillar weights per regime (`qtss-worker/src/confluence.rs`).'
)
ON CONFLICT (key) DO NOTHING;

-- >>> merged from: 0026_external_source_hl_meta_asset_ctxs.sql
-- PLAN §1 / Phase A — Hyperliquid public `metaAndAssetCtxs` (POST). Varsayılan kapalı; açınca worker `external_fetch` tick ile `data_snapshots` yazar.

INSERT INTO external_data_sources (key, enabled, method, url, headers_json, body_json, tick_secs, description)
VALUES (
    'hl_meta_asset_ctxs',
    false,
    'POST',
    'https://api.hyperliquid.xyz/info',
    '{}'::jsonb,
    '{"type": "metaAndAssetCtxs"}'::jsonb,
    120,
    'HL perp universe funding/OI context — büyük JSON; confluence ileride bu anahtarı okuyabilir.'
)
ON CONFLICT (key) DO NOTHING;

-- >>> merged from: 0027_market_confluence_snapshots.sql
-- PLAN Phase B — derived confluence category scores (append-only history per engine target).
-- English JSON keys in scores_json: smart_money, cex_flow, dex_pressure, hyperliquid, funding_oi, liquidations, composite.
-- Idempotent: `0013_worker_analytics_schema.sql` may already create this table (possibly with extra columns).

CREATE TABLE IF NOT EXISTS market_confluence_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    schema_version INT NOT NULL DEFAULT 1,
    regime TEXT,
    composite_score DOUBLE PRECISION NOT NULL,
    confidence_0_100 INT NOT NULL,
    scores_json JSONB NOT NULL,
    conflicts_json JSONB NOT NULL DEFAULT '[]'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_market_confluence_snapshots_symbol_computed
    ON market_confluence_snapshots (engine_symbol_id, computed_at DESC);

COMMENT ON TABLE market_confluence_snapshots IS 'Append-only confluence score history (PLAN_CONFLUENCE_AND_MARKET_DATA Phase B).';

-- >>> merged from: 0028_external_sources_funding_oi_liquidations.sql
-- Binance USDT-M public FAPI: funding + open interest (ücretsiz).
-- Coinglass likidasyon özeti — varsayılan kapalı; URL + API-Key dokümantasyona göre güncellenmeli.

INSERT INTO external_data_sources (key, enabled, method, url, headers_json, body_json, tick_secs, description)
VALUES
(
    'binance_premium_btcusdt',
    true,
    'GET',
    'https://fapi.binance.com/fapi/v1/premiumIndex?symbol=BTCUSDT',
    '{}'::jsonb,
    NULL,
    120,
    'lastFundingRate — confluence funding_oi / onchain karışımı.'
),
(
    'binance_open_interest_btcusdt',
    true,
    'GET',
    'https://fapi.binance.com/fapi/v1/openInterest?symbol=BTCUSDT',
    '{}'::jsonb,
    NULL,
    120,
    'OI seviyesi — zayıf kaldıraç ısısı (zaman serisi olmadan yön sınırlı).'
),
(
    'coinglass_liquidations_btc',
    false,
    'GET',
    'https://open-api.coinglass.com/public/v2/liquidation/info?symbol=BTC',
    '{}'::jsonb,
    NULL,
    300,
    'Likidasyon özeti — Coinglass API-Key: headers_json; endpoint sürümünü https://www.coinglass.com ile doğrulayın.'
)
ON CONFLICT (key) DO NOTHING;

-- >>> merged from: 0029_market_confluence_payload_column.sql
-- Phase B — tam confluence payload kopyası (geçmiş / UI; analysis_snapshots ile çift yazım).

ALTER TABLE market_confluence_snapshots
    ADD COLUMN IF NOT EXISTS confluence_payload_json JSONB;

COMMENT ON COLUMN market_confluence_snapshots.confluence_payload_json IS 'Full confluence engine payload (schema_version 2+) at compute time.';

-- >>> merged from: 0030_onchain_signal_scores.sql
-- SPEC_ONCHAIN_SIGNALS §3.2 — önceki yanlış sürüm: 0014_onchain_signal_scores.sql (0014_acp_auto_scan_timeframe ile çakışıyordu).

CREATE TABLE IF NOT EXISTS onchain_signal_scores (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol TEXT NOT NULL,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    funding_score DOUBLE PRECISION,
    oi_score DOUBLE PRECISION,
    ls_ratio_score DOUBLE PRECISION,
    taker_vol_score DOUBLE PRECISION,
    exchange_netflow_score DOUBLE PRECISION,
    exchange_balance_score DOUBLE PRECISION,
    hl_bias_score DOUBLE PRECISION,
    hl_whale_score DOUBLE PRECISION,
    liquidation_score DOUBLE PRECISION,
    nansen_sm_score DOUBLE PRECISION,
    tvl_trend_score DOUBLE PRECISION,
    aggregate_score DOUBLE PRECISION NOT NULL,
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0.5,
    direction TEXT NOT NULL CHECK (direction IN
        ('strong_buy','buy','neutral','sell','strong_sell')),
    market_regime TEXT,
    conflict_detected BOOLEAN NOT NULL DEFAULT false,
    conflict_detail TEXT,
    snapshot_keys TEXT[] NOT NULL DEFAULT '{}',
    meta_json JSONB
);

CREATE INDEX IF NOT EXISTS idx_ocs_symbol_time ON onchain_signal_scores (symbol, computed_at DESC);

COMMENT ON TABLE onchain_signal_scores IS 'SPEC_ONCHAIN_SIGNALS — qtss-worker onchain_signal_scorer';

-- >>> merged from: 0031_onchain_signal_weights_app_config.sql
-- SPEC_ONCHAIN_SIGNALS §5.5 / §4.2 — bileşen ağırlıkları (`onchain_signal_scorer`).
-- Admin: `PUT /api/v1/config` key `onchain_signal_weights`. Env: `QTSS_ONCHAIN_SIGNAL_WEIGHTS_KEY`.

INSERT INTO app_config (key, value, description)
VALUES (
    'onchain_signal_weights',
    '{
      "taker": 1.0,
      "funding": 1.0,
      "oi": 1.0,
      "ls_ratio": 1.0,
      "coinglass_netflow": 1.0,
      "coinglass_liquidations": 1.0,
      "hl_meta": 1.0,
      "nansen": 1.0
    }'::jsonb,
    'On-chain aggregate: score_i × confidence_i × weight_i / Σ(confidence_i × weight_i) — `qtss-worker/src/onchain_signal_scorer.rs`.'
)
ON CONFLICT (key) DO NOTHING;

-- >>> merged from: 0032_nansen_extended_scores.sql
-- Nansen extended on-chain score columns + weights (QTSS_CURSOR_DEV_GUIDE ADIM 6, §3.2).
-- Requires existing `onchain_signal_scores` table (deploy engine/on-chain migrations first if missing).

ALTER TABLE onchain_signal_scores
    ADD COLUMN IF NOT EXISTS nansen_netflow_score DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS nansen_perp_score DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS nansen_buyer_quality_score DOUBLE PRECISION;

UPDATE app_config
SET value = value
    || '{"nansen_netflows": 1.0, "nansen_perp": 1.5, "nansen_buyer_quality": 0.8, "nansen_flow_intelligence": 1.0}'::jsonb
WHERE key = 'onchain_signal_weights';

INSERT INTO app_config (key, value, description)
VALUES (
    'nansen_whale_watchlist',
    '{"wallets": [], "last_updated": null}'::jsonb,
    'Whale wallet list filled from perp-leaderboard (worker pipeline)'
)
ON CONFLICT (key) DO NOTHING;

-- >>> merged from: 0033_onchain_weights_hl_whale.sql
-- `hl_whale` weight + optional Nansen flow-intel symbol map (QTSS_CURSOR_DEV_GUIDE).

UPDATE app_config
SET value = value || '{"hl_whale": 1.0}'::jsonb
WHERE key = 'onchain_signal_weights';

INSERT INTO app_config (key, value, description)
VALUES (
    'nansen_flow_intel_by_symbol',
    '{}'::jsonb,
    'Per-symbol JSON bodies for Nansen tgm/flow-intelligence (see worker nansen_extended)'
)
ON CONFLICT (key) DO NOTHING;

-- >>> merged from: 0034_engine_symbols_fk_columns.sql
-- Bazı kurulumlarda 0014 uygulanmamış; `engine_symbols.exchange_id` vb. eksik kalınca worker sorguları kırılır.
-- `bar_intervals` henüz yoksa (0013 atlanmış DB’ler) tek ALTER içinde REFERENCES tüm ifadeyi düşürürdü; bu yüzden
-- `exchange_id` / `market_id` / `instrument_id` ayrı, `bar_interval_id` yalnızca `bar_intervals` varsa eklenir.

DO $$
BEGIN
  IF to_regclass('public.engine_symbols') IS NOT NULL THEN
    ALTER TABLE engine_symbols
      ADD COLUMN IF NOT EXISTS exchange_id UUID REFERENCES exchanges (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS market_id UUID REFERENCES markets (id) ON DELETE SET NULL,
      ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL;

    CREATE INDEX IF NOT EXISTS idx_engine_symbols_exchange_id ON engine_symbols (exchange_id);
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_market_id ON engine_symbols (market_id);
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_instrument_id ON engine_symbols (instrument_id);

    IF to_regclass('public.bar_intervals') IS NOT NULL THEN
      ALTER TABLE engine_symbols
        ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL;
      CREATE INDEX IF NOT EXISTS idx_engine_symbols_bar_interval_id ON engine_symbols (bar_interval_id);
    END IF;
  END IF;
END $$;

-- >>> merged from: 0035_engine_symbols_bar_interval_fk_if_ready.sql
-- 0034, `bar_intervals` henüz yokken uygulandıysa `bar_interval_id` eklenmemiş olabilir.
-- `bar_intervals` oluşturulduktan sonra (0013 içeriği veya el ile) bu migrasyon idempotent tamamlar.

DO $$
BEGIN
  IF to_regclass('public.engine_symbols') IS NOT NULL
     AND to_regclass('public.bar_intervals') IS NOT NULL THEN
    ALTER TABLE engine_symbols
      ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL;
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_bar_interval_id ON engine_symbols (bar_interval_id);
  END IF;
END $$;

-- >>> merged from: 0036_bar_intervals_repair_if_missing.sql
-- Telafi: `_sqlx_migrations` içinde 0013 uygulanmış görünüp `public.bar_intervals` yoksa (eski dosya, el ile silme, yarım transaction).
-- Uygulanmış 0013 içeriğini değiştirmeyin; bu dosya idempotent tamamlar. Yeni kurulumlarda çoğu adım no-op.

ALTER TABLE markets DROP CONSTRAINT IF EXISTS markets_segment_check;

ALTER TABLE market_bars
    ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS bar_interval_id UUID;

CREATE TABLE IF NOT EXISTS bar_intervals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL,
    label TEXT,
    duration_seconds INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT bar_intervals_code_key UNIQUE (code)
);

DO $$
BEGIN
    ALTER TABLE market_bars
        ADD CONSTRAINT market_bars_bar_interval_id_fkey FOREIGN KEY (bar_interval_id) REFERENCES bar_intervals (id) ON DELETE SET NULL;
EXCEPTION
    WHEN duplicate_object THEN NULL;
END
$$;

INSERT INTO bar_intervals (code, label, duration_seconds, sort_order, is_active, metadata)
VALUES
    ('1m', '1 minute', 60, 10, TRUE, '{}'),
    ('3m', '3 minutes', 180, 20, TRUE, '{}'),
    ('5m', '5 minutes', 300, 30, TRUE, '{}'),
    ('15m', '15 minutes', 900, 40, TRUE, '{}'),
    ('30m', '30 minutes', 1800, 50, TRUE, '{}'),
    ('1h', '1 hour', 3600, 60, TRUE, '{}'),
    ('2h', '2 hours', 7200, 70, TRUE, '{}'),
    ('4h', '4 hours', 14400, 80, TRUE, '{}'),
    ('1d', '1 day', 86400, 90, TRUE, '{}')
ON CONFLICT (code) DO NOTHING;

INSERT INTO bar_intervals (code, label, duration_seconds, sort_order, is_active, metadata) VALUES
    ('1s', '1 saniye', 1, 5, TRUE, '{}'),
    ('6h', '6 saat', 21600, 65, TRUE, '{}'),
    ('8h', '8 saat', 28800, 70, TRUE, '{}'),
    ('12h', '12 saat', 43200, 75, TRUE, '{}'),
    ('3d', '3 gün', 259200, 85, TRUE, '{}'),
    ('1w', '1 hafta', 604800, 90, TRUE, '{}'),
    ('1M', '1 ay', NULL, 100, TRUE, '{}')
ON CONFLICT (code) DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_bar_intervals_active ON bar_intervals (is_active, sort_order);

COMMENT ON TABLE bar_intervals IS 'OHLC mum aralığı kataloğu; market_bars / engine_symbols FK ile tekrarlayan metin azaltılır.';

UPDATE market_bars mb
SET bar_interval_id = bi.id
FROM bar_intervals bi
WHERE mb.bar_interval_id IS NULL
  AND LOWER(TRIM(mb.interval)) = LOWER(bi.code);

-- 0035, `bar_intervals` yokken uygulanmışsa `bar_interval_id` hiç eklenmemiş olabilir; önce kolon, sonra doldurma.
DO $$
BEGIN
  IF to_regclass('public.engine_symbols') IS NOT NULL
     AND to_regclass('public.bar_intervals') IS NOT NULL THEN
    ALTER TABLE engine_symbols
      ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL;
    CREATE INDEX IF NOT EXISTS idx_engine_symbols_bar_interval_id ON engine_symbols (bar_interval_id);
  END IF;
END $$;

DO $$
BEGIN
  IF to_regclass('public.engine_symbols') IS NOT NULL THEN
    UPDATE engine_symbols es
    SET bar_interval_id = bi.id
    FROM bar_intervals bi
    WHERE es.bar_interval_id IS NULL
      AND LOWER(TRIM(es.interval)) = LOWER(bi.code);
  END IF;
END $$;

-- >>> merged from: 0037_copy_trade_execution_jobs.sql
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

-- >>> merged from: 0038_ai_approval_requests.sql
-- AI / policy human approval queue (dev guide §9.1 item 6).

CREATE TABLE ai_approval_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    requester_user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending',
    kind TEXT NOT NULL DEFAULT 'generic',
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    model_hint TEXT,
    admin_note TEXT,
    decided_by_user_id UUID REFERENCES users (id) ON DELETE SET NULL,
    decided_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT ai_approval_requests_status_chk CHECK (
        status IN (
            'pending',
            'approved',
            'rejected',
            'cancelled'
        )
    )
);

CREATE INDEX idx_ai_approval_org_status_created ON ai_approval_requests (org_id, status, created_at DESC);

-- >>> merged from: 0039_notify_outbox.sql
-- Async notification outbox (beyond direct `qtss-notify` calls) — dev guide §9.1 item 7.

CREATE TABLE notify_outbox (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID REFERENCES organizations (id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    channels JSONB NOT NULL DEFAULT '["webhook"]'::jsonb,
    status TEXT NOT NULL DEFAULT 'pending',
    attempt_count INT NOT NULL DEFAULT 0,
    last_error TEXT,
    sent_at TIMESTAMPTZ,
    delivery_detail JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT notify_outbox_status_chk CHECK (
        status IN (
            'pending',
            'sending',
            'sent',
            'failed'
        )
    )
);

CREATE INDEX idx_notify_outbox_pending_created ON notify_outbox (created_at ASC)
WHERE
    status = 'pending';

-- >>> merged from: 0040_user_permissions.sql
-- Kullanıcı başına ek qtss:* yetenekleri (JWT rol türevi ile birleştirilir; bkz. require_jwt).
-- 0013 öneki `0013_worker_analytics_schema.sql` için ayrılmıştır; çift 0013 kullanmayın (§6).

CREATE TABLE user_permissions (
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, permission)
);

CREATE INDEX idx_user_permissions_user ON user_permissions (user_id);

COMMENT ON TABLE user_permissions IS 'JWT permissions (rol veya claim) ile birleşir; viewer + qtss:ops gibi ek yetki için.';

-- >>> merged from: 0041_audit_log_details.sql
-- RBAC / güvenlik bağlamı (ör. user_permissions önce/sonra). HTTP audit ile aynı tablo.

ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS details JSONB;

COMMENT ON COLUMN audit_log.details IS 'Yapılandırılmış denetim: user_permissions_replace vb.';

-- >>> merged from: 0042_ai_engine_tables.sql
-- FAZ 1 — AI engine core tables (QTSS_MASTER_DEV_GUIDE §4 FAZ 1.1–1.5).
-- Parent: ai_decisions. Children: tactical / position_directives / portfolio_directives / outcomes.

CREATE TABLE ai_decisions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    layer TEXT NOT NULL,
    symbol TEXT,
    model_id TEXT,
    prompt_hash TEXT,
    input_snapshot JSONB NOT NULL DEFAULT '{}'::jsonb,
    raw_output TEXT,
    parsed_decision JSONB,
    status TEXT NOT NULL DEFAULT 'pending_approval',
    approved_by TEXT,
    approved_at TIMESTAMPTZ,
    applied_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    confidence DOUBLE PRECISION,
    meta_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT ai_decisions_layer_chk CHECK (
        layer IN ('strategic', 'tactical', 'operational')
    ),
    CONSTRAINT ai_decisions_status_chk CHECK (
        status IN (
            'pending_approval',
            'approved',
            'applied',
            'rejected',
            'expired',
            'error'
        )
    )
);

CREATE INDEX idx_ai_decisions_symbol_layer_created ON ai_decisions (
    symbol,
    layer,
    created_at DESC
);

CREATE INDEX idx_ai_decisions_status_pending ON ai_decisions (status)
WHERE
    status IN ('pending_approval', 'approved');

CREATE TABLE ai_tactical_decisions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id UUID NOT NULL REFERENCES ai_decisions (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until TIMESTAMPTZ NOT NULL,
    symbol TEXT NOT NULL,
    direction TEXT NOT NULL,
    position_size_multiplier DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    entry_price_hint DOUBLE PRECISION,
    stop_loss_pct DOUBLE PRECISION,
    take_profit_pct DOUBLE PRECISION,
    reasoning TEXT,
    confidence DOUBLE PRECISION,
    status TEXT NOT NULL DEFAULT 'pending_approval',
    CONSTRAINT ai_tactical_direction_chk CHECK (
        direction IN (
            'strong_buy',
            'buy',
            'neutral',
            'sell',
            'strong_sell',
            'no_trade'
        )
    ),
    CONSTRAINT ai_tactical_status_chk CHECK (
        status IN (
            'pending_approval',
            'approved',
            'applied',
            'rejected',
            'expired'
        )
    )
);

CREATE INDEX idx_ai_tactical_symbol_status_created ON ai_tactical_decisions (
    symbol,
    status,
    created_at DESC
);

CREATE INDEX idx_ai_tactical_decision_id ON ai_tactical_decisions (decision_id);

CREATE TABLE ai_position_directives (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id UUID NOT NULL REFERENCES ai_decisions (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    symbol TEXT NOT NULL,
    open_position_ref UUID,
    action TEXT NOT NULL,
    new_stop_loss_pct DOUBLE PRECISION,
    new_take_profit_pct DOUBLE PRECISION,
    trailing_callback_pct DOUBLE PRECISION,
    partial_close_pct DOUBLE PRECISION,
    reasoning TEXT,
    status TEXT NOT NULL DEFAULT 'pending_approval',
    CONSTRAINT ai_position_directives_action_chk CHECK (
        action IN (
            'keep',
            'tighten_stop',
            'widen_stop',
            'activate_trailing',
            'deactivate_trailing',
            'partial_close',
            'full_close',
            'add_to_position'
        )
    ),
    CONSTRAINT ai_position_directives_status_chk CHECK (
        status IN (
            'pending_approval',
            'approved',
            'applied',
            'rejected',
            'expired'
        )
    )
);

CREATE INDEX idx_ai_position_directives_decision_id ON ai_position_directives (decision_id);

CREATE INDEX idx_ai_position_directives_symbol_status_created ON ai_position_directives (
    symbol,
    status,
    created_at DESC
);

CREATE TABLE ai_portfolio_directives (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id UUID NOT NULL REFERENCES ai_decisions (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until TIMESTAMPTZ,
    risk_budget_pct DOUBLE PRECISION,
    max_open_positions INT,
    preferred_regime TEXT,
    symbol_scores JSONB NOT NULL DEFAULT '{}'::jsonb,
    macro_note TEXT,
    status TEXT NOT NULL DEFAULT 'active'
);

CREATE INDEX idx_ai_portfolio_directives_decision_id ON ai_portfolio_directives (decision_id);

CREATE INDEX idx_ai_portfolio_directives_status_created ON ai_portfolio_directives (status, created_at DESC);

CREATE TABLE ai_decision_outcomes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    decision_id UUID NOT NULL REFERENCES ai_decisions (id) ON DELETE CASCADE,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    pnl_pct DOUBLE PRECISION,
    pnl_usdt DOUBLE PRECISION,
    outcome TEXT NOT NULL,
    holding_hours DOUBLE PRECISION,
    notes TEXT,
    CONSTRAINT ai_decision_outcomes_outcome_chk CHECK (
        outcome IN (
            'profit',
            'loss',
            'breakeven',
            'expired_unused'
        )
    )
);

CREATE INDEX idx_ai_decision_outcomes_decision_id ON ai_decision_outcomes (decision_id);

CREATE INDEX idx_ai_decision_outcomes_recorded ON ai_decision_outcomes (recorded_at DESC);

-- >>> merged from: 0043_ai_engine_config.sql
-- FAZ 1.6 — Seed `app_config.ai_engine_config` (idempotent).

INSERT INTO
    app_config (
        key,
        value,
        description,
        updated_by_user_id
    )
VALUES (
        'ai_engine_config',
        '{"enabled": false, "tactical_layer_enabled": true, "operational_layer_enabled": true, "strategic_layer_enabled": false, "auto_approve_threshold": 0.85, "auto_approve_enabled": false, "tactical_tick_secs": 900, "operational_tick_secs": 120, "strategic_tick_secs": 86400, "provider_tactical": "anthropic", "provider_operational": "anthropic", "provider_strategic": "anthropic", "model_tactical": "claude-haiku-4-5-20251001", "model_operational": "claude-haiku-4-5-20251001", "model_strategic": "claude-sonnet-4-20250514", "max_tokens_tactical": 1024, "max_tokens_operational": 512, "max_tokens_strategic": 4096, "decision_ttl_secs": 1800, "require_min_confidence": 0.60}'::jsonb,
        'AI engine defaults (providers + ticks); qtss-ai reads with app_config merge / env overrides.',
        NULL
    )
ON CONFLICT (key) DO NOTHING;

-- >>> merged from: 0044_system_config.sql
-- FAZ 11.1 — Operational parameters in DB (module-scoped); secrets stay in env / secret store (see docs/CONFIG_REGISTRY.md).

CREATE TABLE system_config (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    module TEXT NOT NULL,
    config_key TEXT NOT NULL,
    value JSONB NOT NULL DEFAULT '{}'::jsonb,
    schema_version INT NOT NULL DEFAULT 1,
    description TEXT,
    is_secret BOOLEAN NOT NULL DEFAULT false,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by_user_id UUID REFERENCES users (id),
    CONSTRAINT system_config_module_key_unique UNIQUE (module, config_key)
);

CREATE INDEX idx_system_config_module ON system_config (module);

CREATE INDEX idx_system_config_module_config_key ON system_config (module, config_key);

-- Idempotent seed (non-secret documentation defaults).
INSERT INTO
    system_config (module, config_key, value, description)
VALUES (
        'ai',
        'worker_doc',
        '{"note":"QTSS_AI_ENGINE_WORKER=0 disables qtss-worker AI spawn loops; providers still need keys when enabled."}'::jsonb,
        'AI worker env cross-reference (FAZ 5 / 8).'
    )
ON CONFLICT (module, config_key) DO NOTHING;

-- >>> merged from: 0045_users_preferred_locale_and_worker_ticks.sql
-- FAZ 9.3 — user locale preference; FAZ 11.7 — worker tick seeds (non-secret).

ALTER TABLE users ADD COLUMN IF NOT EXISTS preferred_locale TEXT;

COMMENT ON COLUMN users.preferred_locale IS 'BCP47-style language tag: en, tr, or NULL for default.';

INSERT INTO system_config (module, config_key, value, description)
VALUES
    (
        'worker',
        'notify_outbox_tick_secs',
        '{"secs":10}'::jsonb,
        'notify_outbox consumer poll interval (seconds); env QTSS_NOTIFY_OUTBOX_TICK_SECS when QTSS_CONFIG_ENV_OVERRIDES=1 overrides DB.'
    ),
    (
        'worker',
        'pnl_rollup_tick_secs',
        '{"secs":300}'::jsonb,
        'PnL rollup rebuild interval (seconds); env QTSS_PNL_ROLLUP_TICK_SECS; min 60 in worker.'
    ),
    (
        'worker',
        'notify_default_locale',
        '{"code":"tr"}'::jsonb,
        'Default locale for worker bilingual notify copy (en|tr); env QTSS_NOTIFY_DEFAULT_LOCALE.'
    )
ON CONFLICT (module, config_key) DO NOTHING;

-- >>> merged from: 0046_worker_paper_live_notify_tick_secs.sql
-- FAZ 11.7 — paper / live position notify poll intervals in `system_config` (non-secret).

INSERT INTO system_config (module, config_key, value, description)
VALUES
    (
        'worker',
        'paper_position_notify_tick_secs',
        '{"secs":30}'::jsonb,
        'Paper fill notify loop interval (seconds); env QTSS_NOTIFY_POSITION_TICK_SECS; min 10 in worker.'
    ),
    (
        'worker',
        'live_position_notify_tick_secs',
        '{"secs":45}'::jsonb,
        'Live fill notify loop interval (seconds); env QTSS_NOTIFY_LIVE_TICK_SECS; min 15 in worker.'
    )
ON CONFLICT (module, config_key) DO NOTHING;

-- >>> merged from: 0047_worker_kill_switch_tick_secs.sql
-- FAZ 11.7 — kill_switch DB sync + PnL poll intervals in system_config (non-secret).

INSERT INTO system_config (module, config_key, value, description)
VALUES
    (
        'worker',
        'kill_switch_db_sync_tick_secs',
        '{"secs":5}'::jsonb,
        'Poll app_config kill_switch_trading_halted for in-process halt flag; env QTSS_KILL_SWITCH_DB_SYNC_SECS; min 2.'
    ),
    (
        'worker',
        'kill_switch_pnl_poll_tick_secs',
        '{"secs":60}'::jsonb,
        'kill_switch_loop PnL check interval when QTSS_KILL_SWITCH_ENABLED; env QTSS_KILL_SWITCH_TICK_SECS; min 15.'
    )
ON CONFLICT (module, config_key) DO NOTHING;

-- >>> merged from: 0048_exchange_fills.sql
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


-- >>> merged from: 0049_pnl_rollups_closed_trade_count.sql
ALTER TABLE pnl_rollups
ADD COLUMN closed_trade_count BIGINT NOT NULL DEFAULT 0;


-- >>> merged from: 0050_notify_outbox_meta.sql
-- Enrich notify_outbox: filterable metadata fields (event_key / instrument).

ALTER TABLE notify_outbox
ADD COLUMN event_key TEXT,
ADD COLUMN severity TEXT NOT NULL DEFAULT 'info',
ADD COLUMN exchange TEXT,
ADD COLUMN segment TEXT,
ADD COLUMN symbol TEXT;

ALTER TABLE notify_outbox
ADD CONSTRAINT notify_outbox_severity_chk CHECK (severity IN ('info', 'warn', 'error'));

CREATE INDEX idx_notify_outbox_org_created ON notify_outbox (org_id, created_at DESC);
CREATE INDEX idx_notify_outbox_event_key ON notify_outbox (event_key);
CREATE INDEX idx_notify_outbox_instrument ON notify_outbox (exchange, segment, symbol);


-- >>> merged from: 0051_pnl_rollups_segment.sql
-- Add `segment` to pnl_rollups to avoid spot/futures mixing.

ALTER TABLE pnl_rollups
ADD COLUMN segment TEXT;

-- Backfill legacy rows (best effort). Live rebuild will overwrite live ledger anyway.
UPDATE pnl_rollups
SET segment = COALESCE(NULLIF(segment, ''), 'spot')
WHERE segment IS NULL OR segment = '';

ALTER TABLE pnl_rollups
ALTER COLUMN segment SET NOT NULL;

-- Primary key must include segment.
ALTER TABLE pnl_rollups
DROP CONSTRAINT IF EXISTS pnl_rollups_pkey;

ALTER TABLE pnl_rollups
ADD PRIMARY KEY (org_id, exchange, segment, symbol, ledger, bucket, period_start);

DROP INDEX IF EXISTS idx_pnl_ledger_bucket;
CREATE INDEX idx_pnl_ledger_bucket ON pnl_rollups (ledger, bucket, period_start DESC);
CREATE INDEX idx_pnl_instrument ON pnl_rollups (exchange, segment, symbol);


-- >>> merged from: 0052_worker_feature_flags_seed.sql
-- Worker feature flags (UI checkbox support) — safe defaults.

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES
  ('worker', 'nansen_enabled', '{"enabled": false}', 'Enable Nansen HTTP loops (prevents credit burn when false).', false),
  ('worker', 'external_fetch_enabled', '{"enabled": true}', 'Enable external_data_sources HTTP engines.', false)
ON CONFLICT (module, config_key) DO NOTHING;


-- >>> merged from: 0053_system_config_extra_seeds.sql
-- Additional `system_config` seeds (non-secret).
-- Keep secrets in env (DATABASE_URL, QTSS_JWT_SECRET, provider API keys).

INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret)
VALUES
    -- API
    ('api', 'bind', '{"value":"0.0.0.0:8080"}'::jsonb, 1, 'HTTP bind address for qtss-api.', false),
    ('api', 'jwt_audience', '{"value":"qtss-api"}'::jsonb, 1, 'JWT aud claim.', false),
    ('api', 'jwt_issuer', '{"value":"qtss"}'::jsonb, 1, 'JWT iss claim.', false),
    ('api', 'jwt_access_ttl_secs', '{"value":"900"}'::jsonb, 1, 'JWT access token TTL seconds.', false),
    ('api', 'jwt_refresh_ttl_secs', '{"value":"2592000"}'::jsonb, 1, 'JWT refresh token TTL seconds.', false),
    ('api', 'rate_limit_replenish_ms', '{"value":"20"}'::jsonb, 1, 'tower-governor replenish (ms).', false),
    ('api', 'rate_limit_burst', '{"value":"120"}'::jsonb, 1, 'tower-governor burst size.', false),
    ('api', 'audit_http_enabled', '{"enabled": false}'::jsonb, 1, 'Enable audit_log for HTTP mutations (requires admin).', false),
    ('api', 'trusted_proxies_csv', '{"value":""}'::jsonb, 1, 'Comma-separated trusted proxy IP/CIDR list (ForwardedIpKeyExtractor).', false),
    ('api', 'metrics_token', '{"value":""}'::jsonb, 1, 'Optional token required for GET /metrics.', false),
    ('seed', 'admin_email', '{"value":"admin@localhost"}'::jsonb, 1, 'Default admin email for qtss-seed.', false),

    -- Worker: kline and probe HTTP
    ('worker', 'kline_interval', '{"value":"1m"}'::jsonb, 1, 'Kline interval for Binance WS (1m/15m/4h...).', false),
    ('worker', 'kline_segment', '{"value":"spot"}'::jsonb, 1, 'Kline segment for Binance WS (spot|futures).', false),
    ('worker', 'kline_symbol', '{"value":""}'::jsonb, 1, 'Single kline symbol (e.g., BTCUSDT) when kline_symbols is empty.', false),
    ('worker', 'kline_symbols_csv', '{"value":""}'::jsonb, 1, 'Comma-separated symbols for combined kline WS.', false),
    ('worker', 'http_bind', '{"value":""}'::jsonb, 1, 'Optional worker probe HTTP bind address (e.g., 127.0.0.1:9090).', false),

    -- Worker: notify feature flags and channels (credentials remain env for qtss-notify)
    ('worker', 'notify_outbox_enabled', '{"enabled": false}'::jsonb, 1, 'Enable notify_outbox consumer loop.', false),
    ('worker', 'notify_outbox_tick_secs', '{"secs": 10}'::jsonb, 1, 'notify_outbox consumer tick seconds.', false),
    ('worker', 'paper_position_notify_enabled', '{"enabled": false}'::jsonb, 1, 'Enable paper (dry) fill notifications.', false),
    ('worker', 'paper_position_notify_channels_csv', '{"value":"telegram"}'::jsonb, 1, 'Channels for paper fill notifications.', false),
    ('worker', 'paper_position_notify_tick_secs', '{"secs": 30}'::jsonb, 1, 'Paper fill notify loop tick seconds.', false),
    ('worker', 'live_position_notify_enabled', '{"enabled": false}'::jsonb, 1, 'Enable live exchange fill notifications.', false),
    ('worker', 'live_position_notify_channels_csv', '{"value":"telegram"}'::jsonb, 1, 'Channels for live fill notifications.', false),
    ('worker', 'live_position_notify_tick_secs', '{"secs": 45}'::jsonb, 1, 'Live fill notify loop tick seconds.', false),

    -- Worker: kill switch (numbers stored as strings; parsed in worker)
    ('worker', 'kill_switch_enabled', '{"enabled": false}'::jsonb, 1, 'Enable kill switch loop.', false),
    ('worker', 'kill_switch_db_sync_tick_secs', '{"secs": 5}'::jsonb, 1, 'Kill switch app_config sync poll tick.', false),
    ('worker', 'kill_switch_pnl_poll_tick_secs', '{"secs": 60}'::jsonb, 1, 'Kill switch PnL poll tick.', false),
    ('worker', 'kill_switch_reference_equity_usdt', '{"value":"100000"}'::jsonb, 1, 'Reference equity for drawdown percentage.', false),
    ('worker', 'max_drawdown_pct', '{"value":"5.0"}'::jsonb, 1, 'Max drawdown percent (e.g., 5.0).', false),
    ('worker', 'kill_switch_daily_loss_usdt', '{"value":"1000000"}'::jsonb, 1, 'Daily loss trigger (USDT) if drawdown pct is not set.', false)
ON CONFLICT (module, config_key) DO NOTHING;


-- >>> merged from: 0054_system_config_jwt_and_seed.sql
-- Add system_config keys for JWT and seed bootstrap (non-secret values).
-- NOTE: Do not store DATABASE_URL in DB (bootstrap). Secrets are persisted as `is_secret=true`.

INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret)
VALUES
    ('api', 'jwt_audience', '{"value":"qtss-api"}'::jsonb, 1, 'JWT aud claim.', false),
    ('api', 'jwt_issuer', '{"value":"qtss"}'::jsonb, 1, 'JWT iss claim.', false),
    ('api', 'jwt_access_ttl_secs', '{"value":"900"}'::jsonb, 1, 'JWT access token TTL seconds.', false),
    ('api', 'jwt_refresh_ttl_secs', '{"value":"2592000"}'::jsonb, 1, 'JWT refresh token TTL seconds.', false),
    ('seed', 'admin_email', '{"value":"admin@localhost"}'::jsonb, 1, 'Default admin email for qtss-seed.', false)
ON CONFLICT (module, config_key) DO NOTHING;


-- >>> merged from: 0055_system_config_nansen.sql
-- Nansen settings moved to system_config (secrets masked).

INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret)
VALUES
    ('worker', 'nansen_api_base', '{"value":"https://api.nansen.ai"}'::jsonb, 1, 'Nansen API base URL.', false),
    ('worker', 'nansen_api_key', '{"value":""}'::jsonb, 1, 'Nansen API key (secret).', true),
    ('worker', 'nansen_insufficient_credits_sleep_secs', '{"secs":3600}'::jsonb, 1, 'Sleep seconds after insufficient credits (403).', false)
ON CONFLICT (module, config_key) DO NOTHING;


-- >>> merged from: 0056_paper_ledger_strategy_key.sql
-- Paper ledger: per-strategy rows for parallel dry runners (`PaperRecordingDryGateway`).
-- Baseline tables: `0017_paper_ledger.sql` (user_id PK, fills without strategy_key).
-- Idempotent: safe if re-run or if composite PK already exists.

ALTER TABLE paper_balances
    ADD COLUMN IF NOT EXISTS strategy_key TEXT NOT NULL DEFAULT 'default';

DO $$
BEGIN
    ALTER TABLE paper_balances DROP CONSTRAINT paper_balances_pkey;
EXCEPTION
    WHEN undefined_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE paper_balances ADD PRIMARY KEY (user_id, strategy_key);
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

CREATE INDEX IF NOT EXISTS idx_paper_balances_org ON paper_balances (org_id);

ALTER TABLE paper_fills
    ADD COLUMN IF NOT EXISTS strategy_key TEXT NOT NULL DEFAULT 'default';

CREATE INDEX IF NOT EXISTS idx_paper_fills_strategy ON paper_fills (user_id, strategy_key, created_at DESC);

-- >>> merged from: 0057_ai_decisions_approval_request_fk.sql
-- Link `ai_decisions` to `ai_approval_requests` (was part of wrongly numbered `0013_ai_*`, duplicate of v13).
-- `0038_ai_approval_requests.sql` + `0042_ai_engine_tables.sql` own the base tables; this is the delta only.
-- Idempotent: safe if the column already exists (e.g. DB that applied the old `0013_ai` file).

ALTER TABLE ai_decisions
    ADD COLUMN IF NOT EXISTS approval_request_id UUID REFERENCES ai_approval_requests (id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_ai_decisions_approval_request ON ai_decisions (approval_request_id)
WHERE
    approval_request_id IS NOT NULL;

-- >>> merged from: 0058_system_config_runtime_seeds.sql
-- FAZ 11: Extra `system_config` / `app_config` seeds (idempotent).
-- Schema: `0044_system_config.sql`. Env fallback: `QTSS_CONFIG_ENV_OVERRIDES=1`.
-- Not: Duplicated `0013_*.sql` önekleri SQLx sürüm çakışması yaratır; worker çekirdeği `0013_worker_analytics_schema.sql` kalır.

-- api
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('api', 'jwt_audience', '{"value":"qtss-api"}', 'JWT aud', false),
('api', 'jwt_issuer', '{"value":"qtss"}', 'JWT iss', false),
('api', 'jwt_access_ttl_secs', '{"value":"900"}', 'Access TTL seconds', false),
('api', 'jwt_refresh_ttl_secs', '{"value":"2592000"}', 'Refresh TTL seconds', false),
('api', 'bind', '{"value":"0.0.0.0:8080"}', 'HTTP bind', false),
('api', 'rate_limit_replenish_ms', '{"value":"20"}', 'Governor replenish ms', false),
('api', 'rate_limit_burst', '{"value":"120"}', 'Governor burst', false),
('api', 'metrics_token', '{"value":""}', 'Optional /metrics token', false),
('api', 'trusted_proxies_csv', '{"value":""}', 'Trusted reverse proxies CIDR list', false),
('api', 'audit_http_enabled', '{"enabled":false}', 'HTTP mutation audit_log', false)
ON CONFLICT (module, config_key) DO NOTHING;

-- seed (qtss-seed — admin_password / oauth_client_secret satırları seed tarafından oluşturulur)
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('seed', 'admin_email', '{"value":"admin@localhost"}', 'Admin email', false),
('seed', 'binance_spot_api_key', '{"value":""}', 'Optional seed → exchange_accounts', true),
('seed', 'binance_spot_api_secret', '{"value":""}', 'Optional seed → exchange_accounts', true),
('seed', 'binance_futures_api_key', '{"value":""}', 'Optional seed → exchange_accounts', true),
('seed', 'binance_futures_api_secret', '{"value":""}', 'Optional seed → exchange_accounts', true)
ON CONFLICT (module, config_key) DO NOTHING;

-- worker — kline / rollup / Nansen
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('worker', 'pnl_rollup_tick_secs', '{"secs":300}', 'PnL rollup loop', false),
('worker', 'market_data_exchange', '{"value":"binance"}', 'Kline venue', false),
('worker', 'kline_interval', '{"value":"1m"}', 'Default kline interval', false),
('worker', 'kline_segment', '{"value":"spot"}', 'Default kline segment', false),
('worker', 'kline_symbols_csv', '{"value":""}', 'Comma symbols override', false),
('worker', 'kline_symbol', '{"value":""}', 'Single symbol fallback', false),
('worker', 'http_bind', '{"value":""}', 'Worker probe HTTP bind', false),
('worker', 'nansen_enabled', '{"enabled":true}', 'Nansen master switch', false),
('worker', 'nansen_token_screener_tick_secs', '{"secs":1800}', 'Token screener poll', false),
('worker', 'nansen_insufficient_credits_sleep_secs', '{"secs":3600}', '402/403 backoff', false),
('worker', 'nansen_api_base', '{"value":"https://api.nansen.ai"}', 'Nansen REST base', false),
('worker', 'nansen_api_key', '{"value":""}', 'Nansen API key', true),
('worker', 'notify_outbox_enabled', '{"enabled":false}', 'Drain notify_outbox', false),
('worker', 'notify_outbox_tick_secs', '{"secs":10}', 'Outbox poll', false),
('worker', 'kill_switch_db_sync_tick_secs', '{"secs":5}', 'app_config halt sync', false),
('worker', 'kill_switch_enabled', '{"enabled":false}', 'P&L-based kill loop', false),
('worker', 'kill_switch_pnl_poll_tick_secs', '{"secs":60}', 'Kill loop poll', false),
('worker', 'max_drawdown_pct', '{"value":"5.0"}', 'Drawdown % vs reference equity', false),
('worker', 'kill_switch_reference_equity_usdt', '{"value":"100000"}', 'Reference equity USDT', false),
('worker', 'kill_switch_daily_loss_usdt', '{"value":"1000000"}', 'Daily loss cap (if no drawdown %)', false),
('worker', 'position_manager_enabled', '{"enabled":false}', 'Position manager', false),
('worker', 'position_manager_tick_secs', '{"secs":10}', 'Position manager poll', false),
('worker', 'position_manager_bar_interval', '{"value":"1m"}', 'Mark price from bars', false),
('worker', 'position_manager_dry_close_enabled', '{"enabled":false}', 'Dry reduce-only exit', false),
('worker', 'position_manager_live_close_enabled', '{"enabled":false}', 'Live reduce-only exit', false),
('worker', 'position_manager_trailing_on_directive', '{"enabled":false}', 'Trailing only on AI directive', false),
('worker', 'position_manager_managed_trailing_enabled', '{"enabled":false}', 'Managed trailing stops', false),
('worker', 'position_manager_managed_trailing_callback_rate_pct', '{"value":"1"}', 'Trailing callback %', false),
('worker', 'position_manager_managed_trailing_limit_offset_pct', '{"value":"0.2"}', 'Limit offset %', false),
('worker', 'position_manager_managed_trailing_replace_step_pct', '{"value":"0.1"}', 'Replace step %', false),
('worker', 'default_stop_loss_pct', '{"value":"2.0"}', 'Default SL %', false),
('worker', 'default_take_profit_pct', '{"value":"4.0"}', 'Default TP %', false),
('worker', 'default_leverage', '{"value":"3"}', 'Default leverage hint', false),
('worker', 'reconcile_binance_spot_enabled', '{"enabled":false}', 'Spot open-order reconcile', false),
('worker', 'reconcile_binance_spot_tick_secs', '{"secs":3600}', 'Spot reconcile poll', false),
('worker', 'reconcile_binance_spot_patch_status', '{"enabled":true}', 'Patch submitted→reconciled_not_open', false),
('worker', 'reconcile_binance_spot_refine_order_status', '{"enabled":false}', 'GET /order refine', false),
('worker', 'reconcile_binance_spot_refine_max', '{"value":"30"}', 'Max refine queries', false),
('worker', 'reconcile_binance_futures_enabled', '{"enabled":false}', 'Futures reconcile', false),
('worker', 'reconcile_binance_futures_tick_secs', '{"secs":3600}', 'Futures reconcile poll', false),
('worker', 'reconcile_binance_futures_patch_status', '{"enabled":true}', 'Futures patch status', false),
('worker', 'reconcile_binance_futures_refine_order_status', '{"enabled":false}', 'Futures refine', false),
('worker', 'reconcile_binance_futures_refine_max', '{"value":"30"}', 'Futures refine max', false),
('worker', 'ai_expire_stale_decisions_tick_secs', '{"secs":300}', 'Expire stale AI rows', false),
('worker', 'ai_engine_worker_enabled', '{"enabled":true}', 'Spawn qtss-ai background tasks in worker', false),
('worker', 'strategy_runner_enabled', '{"enabled":false}', 'Dry strategy runner', false),
('worker', 'paper_ledger_enabled', '{"enabled":false}', 'Paper ledger persist', false),
('worker', 'paper_org_id', '{"value":""}', 'Paper org UUID', false),
('worker', 'paper_user_id', '{"value":""}', 'Paper user UUID', false),
('worker', 'strategy_runner_quote_balance_usdt', '{"value":"100000"}', 'Total dry quote balance', false),
('worker', 'strategy_signal_filter_balance', '{"value":""}', 'Optional per-strategy balance', false),
('worker', 'strategy_whale_momentum_balance', '{"value":""}', 'Optional per-strategy balance', false),
('worker', 'strategy_arb_funding_balance', '{"value":""}', 'Optional per-strategy balance', false),
('worker', 'strategy_copy_trade_balance', '{"value":""}', 'Optional per-strategy balance', false)
ON CONFLICT (module, config_key) DO NOTHING;

-- strategy (qtss-strategy dry runner)
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('strategy', 'signal_filter_tick_secs', '{"secs":60}', 'signal_filter poll', false),
('strategy', 'signal_filter_auto_place', '{"enabled":false}', NULL, false),
('strategy', 'signal_filter_bracket_orders', '{"enabled":false}', NULL, false),
('strategy', 'strategy_order_qty', '{"value":"0.001"}', NULL, false),
('strategy', 'strategy_skip_human_approval', '{"enabled":false}', NULL, false),
('strategy', 'long_threshold', '{"value":"0.6"}', NULL, false),
('strategy', 'short_threshold', '{"value":"-0.6"}', NULL, false),
('strategy', 'min_signal_confidence', '{"value":"0.4"}', NULL, false),
('strategy', 'signal_filter_on_conflict', '{"value":"skip"}', 'skip|half', false),
('strategy', 'max_position_notional_usdt', '{"value":"10000"}', NULL, false),
('strategy', 'kelly_apply', '{"enabled":false}', NULL, false),
('strategy', 'kelly_win_rate', '{"value":"0.55"}', NULL, false),
('strategy', 'kelly_avg_win_loss_ratio', '{"value":"1.5"}', NULL, false),
('strategy', 'kelly_max_fraction', '{"value":"0.25"}', NULL, false),
('strategy', 'max_drawdown_pct', '{"value":"5.0"}', 'Drawdown guard', false),
('strategy', 'whale_momentum_tick_secs', '{"secs":120}', NULL, false),
('strategy', 'whale_momentum_threshold', '{"value":"0.45"}', NULL, false),
('strategy', 'whale_funding_crowding_block', '{"value":"0.0002"}', NULL, false),
('strategy', 'whale_momentum_auto_place', '{"enabled":false}', NULL, false),
('strategy', 'copy_trade_strategy_tick_secs', '{"secs":120}', NULL, false),
('strategy', 'copy_trade_direction_threshold', '{"value":"0.25"}', NULL, false),
('strategy', 'copy_trade_base_qty', '{"value":"0.001"}', NULL, false),
('strategy', 'copy_trade_default_symbol', '{"value":"BTCUSDT"}', NULL, false),
('strategy', 'copy_trade_bar_exchange', '{"value":"binance"}', NULL, false),
('strategy', 'copy_trade_bar_segment', '{"value":"futures"}', NULL, false),
('strategy', 'copy_trade_bar_interval', '{"value":"1m"}', NULL, false),
('strategy', 'copy_trade_strategy_auto_place', '{"enabled":false}', NULL, false),
('strategy', 'arb_funding_tick_secs', '{"secs":300}', NULL, false),
('strategy', 'arb_funding_threshold', '{"value":"0.0001"}', NULL, false),
('strategy', 'arb_funding_dry_two_leg', '{"enabled":false}', NULL, false),
('strategy', 'arb_funding_order_qty', '{"value":"0.001"}', NULL, false),
('strategy', 'arb_funding_symbol_base', '{"value":"btc"}', NULL, false),
('strategy', 'default_stop_loss_pct', '{"value":"2.0"}', 'Shared SL % for signal_filter', false),
('strategy', 'default_take_profit_pct', '{"value":"4.0"}', 'Shared TP % for signal_filter', false)
ON CONFLICT (module, config_key) DO NOTHING;

-- notify — qtss-notify JSON (`NotifyConfig`); kanalları admin API ile güncelleyin
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('notify', 'dispatcher_config', '{"telegram":null,"email":null,"sms":null,"whatsapp":null,"x":null,"facebook":null,"instagram":null,"discord":null,"webhook":null}', 'NotificationDispatcher channels', false)
ON CONFLICT (module, config_key) DO NOTHING;

-- ai — provider uçları (anahtarlar gizli)
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('ai', 'anthropic_api_key', '{"value":""}', 'ANTHROPIC_API_KEY', true),
('ai', 'anthropic_base_url', '{"value":"https://api.anthropic.com"}', NULL, false),
('ai', 'anthropic_timeout_secs', '{"secs":120}', NULL, false),
('ai', 'ollama_base_url', '{"value":"http://127.0.0.1:11434"}', NULL, false),
('ai', 'openai_compat_base_url', '{"value":""}', 'OpenAI-compatible /v1 base', false),
('ai', 'openai_compat_headers_json', '{"value":""}', 'Extra JSON headers', false),
('ai', 'onprem_timeout_secs', '{"secs":180}', NULL, false),
('ai', 'onprem_max_in_flight', '{"value":"4"}', NULL, false),
('ai', 'onprem_api_key', '{"value":""}', 'Optional Bearer for gateway', true)
ON CONFLICT (module, config_key) DO NOTHING;

-- app_config: ai_engine_config (tek satır JSON)
INSERT INTO app_config (key, value, description)
VALUES (
    'ai_engine_config',
    '{"enabled":false,"tactical_layer_enabled":true,"operational_layer_enabled":true,"strategic_layer_enabled":false,"auto_approve_threshold":0.85,"auto_approve_enabled":false,"tactical_tick_secs":900,"operational_tick_secs":120,"strategic_tick_secs":86400,"provider_tactical":"anthropic","provider_operational":"anthropic","provider_strategic":"anthropic","model_tactical":"claude-haiku-4-5-20251001","model_operational":"claude-haiku-4-5-20251001","model_strategic":"claude-sonnet-4-20250514","max_tokens_tactical":1024,"max_tokens_operational":512,"max_tokens_strategic":4096,"decision_ttl_secs":1800,"require_min_confidence":0.60,"output_locale":null}'::jsonb,
    'AI engine defaults (merged with system_config ai.* secrets at runtime)'
)
ON CONFLICT (key) DO NOTHING;

-- >>> merged from: 0059_pnl_rollups_segment_closed_trade.sql
-- P&L rollup şeması: `qtss-storage::pnl` (segment filtre, rebuild INSERT, closed_trade_count).

ALTER TABLE pnl_rollups
    ADD COLUMN IF NOT EXISTS segment TEXT NOT NULL DEFAULT 'spot';

ALTER TABLE pnl_rollups
    ADD COLUMN IF NOT EXISTS closed_trade_count BIGINT NOT NULL DEFAULT 0;

-- Yeni PK segment içerir; NULL sembol tekilleştirmesi için boş metin.
UPDATE pnl_rollups SET symbol = '' WHERE symbol IS NULL;

ALTER TABLE pnl_rollups
    ALTER COLUMN symbol SET NOT NULL,
    ALTER COLUMN symbol SET DEFAULT '';

ALTER TABLE pnl_rollups DROP CONSTRAINT IF EXISTS pnl_rollups_pkey;

ALTER TABLE pnl_rollups ADD CONSTRAINT pnl_rollups_pkey PRIMARY KEY (
    org_id,
    exchange,
    segment,
    symbol,
    ledger,
    bucket,
    period_start
);

-- >>> merged from: 0060_notify_engine_analysis_flags.sql
-- Engine analysis notify toggles (`qtss-analysis` `engine_loop`): DB-first, env fallback (`QTSS_*`), `QTSS_CONFIG_ENV_OVERRIDES=1` env wins.
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('notify', 'notify_on_sweep', '{"enabled":false}', 'Sweep edge Telegram/webhook from engine loop', false),
('notify', 'notify_on_sweep_channels', '{"value":"webhook"}', 'Comma channel list for sweep notify', false),
('notify', 'notify_on_range_events', '{"enabled":false}', 'Trading range setup + range_signal_events notify', false),
('notify', 'notify_on_range_events_channels', '{"value":"telegram"}', 'Comma channel list for range events', false)
ON CONFLICT (module, config_key) DO NOTHING;

-- >>> merged from: 0061_engine_symbols_futures_btc_eth_trading_range.sql
-- Binance USDT-M futures: Trading Range motoru (`trading_range` + `signal_dashboard`) için hedefler.
-- `signal_direction_mode = both` → LONG/SHORT (çift yönlü); spot’ta genelde `auto_segment` / long_only kalır.
-- `sort_order` düşük olan satır kline WebSocket fallback’inde önce gelir (`list_enabled_engine_symbols` → worker ilk satırdan interval/segment alır).
INSERT INTO engine_symbols (exchange, segment, symbol, interval, enabled, sort_order, label, signal_direction_mode)
VALUES
  ('binance', 'futures', 'BTCUSDT', '15m', true, -100, 'Futures TR BTC 15m', 'both'),
  ('binance', 'futures', 'ETHUSDT', '15m', true, -99, 'Futures TR ETH 15m', 'both')
ON CONFLICT (exchange, segment, symbol, interval) DO UPDATE SET
  enabled = EXCLUDED.enabled,
  sort_order = EXCLUDED.sort_order,
  label = COALESCE(EXCLUDED.label, engine_symbols.label),
  signal_direction_mode = EXCLUDED.signal_direction_mode,
  updated_at = now();

-- >>> merged from: 0062_range_signal_paper_executions.sql
-- Idempotent paper execution for range_signal_events (worker optional loop).
CREATE TABLE IF NOT EXISTS range_signal_paper_executions (
    range_signal_event_id UUID PRIMARY KEY REFERENCES range_signal_events (id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    client_order_id UUID,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_range_signal_paper_executions_status_created
    ON range_signal_paper_executions (status, created_at DESC);

-- >>> merged from: 0063_worker_catalog_sync_config.sql
-- Binance katalog senkronu (worker): `instruments` + delist sonrası `is_trading=false`

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES
  (
    'worker',
    'catalog_sync_enabled',
    '{"enabled": true}',
    'Enable periodic Binance exchangeInfo → exchanges/markets/instruments sync.',
    false
  ),
  (
    'worker',
    'catalog_sync_tick_secs',
    '{"secs": 3600}',
    'Seconds between Binance catalog sync runs (minimum enforced in code).',
    false
  )
ON CONFLICT (module, config_key) DO NOTHING;

-- >>> squashed from: 0002_engine_symbol_ingestion_state.sql
-- Worker REST backfill health (`qtss-storage` ingestion_state, `GET …/analysis/engine/ingestion-state`).

CREATE TABLE IF NOT EXISTS engine_symbol_ingestion_state (
    engine_symbol_id UUID NOT NULL PRIMARY KEY REFERENCES engine_symbols (id) ON DELETE CASCADE,
    bar_row_count INTEGER NOT NULL DEFAULT 0,
    min_open_time TIMESTAMPTZ,
    max_open_time TIMESTAMPTZ,
    gap_count INTEGER NOT NULL DEFAULT 0,
    max_gap_seconds INTEGER,
    last_backfill_at TIMESTAMPTZ,
    last_health_check_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- >>> squashed from: 0003_telegram_setup_analysis_system_config.sql
-- Telegram setup-analysis webhook + Gemini: `system_config` module `telegram_setup_analysis`.

INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret)
VALUES
    (
        'telegram_setup_analysis',
        'trigger_phrase',
        '{"value":"QTSS_ANALIZ"}'::jsonb,
        1,
        'User sends this phrase (alone or with a trailing note) to flush the queue and run analysis.',
        false
    ),
    (
        'telegram_setup_analysis',
        'gemini_model',
        '{"value":"gemini-2.5-flash"}'::jsonb,
        1,
        'Gemini model id for generateContent (Google AI).',
        false
    ),
    (
        'telegram_setup_analysis',
        'webhook_secret',
        '{"value":""}'::jsonb,
        1,
        'Path secret for POST /telegram/setup-analysis/{secret}. Non-empty enables the webhook.',
        true
    ),
    (
        'telegram_setup_analysis',
        'gemini_api_key',
        '{"value":""}'::jsonb,
        1,
        'Google AI Studio / Gemini API key.',
        true
    ),
    (
        'telegram_setup_analysis',
        'max_buffer_turns',
        '{"value":"12"}'::jsonb,
        1,
        'Max queued items per chat (1–50).',
        false
    ),
    (
        'telegram_setup_analysis',
        'buffer_ttl_secs',
        '{"value":"7200"}'::jsonb,
        1,
        'Drop stale queue entries older than this many seconds (300–86400).',
        false
    ),
    (
        'telegram_setup_analysis',
        'allowed_chat_ids',
        '{"value":""}'::jsonb,
        1,
        'Optional comma-separated Telegram chat ids; empty = allow all chats.',
        false
    )
ON CONFLICT (module, config_key) DO NOTHING;

-- >>> squashed from: 0005_notify_telegram_system_config_seed.sql
-- `notify.telegram_*` for `load_notify_config_merged` (optional; fill via Admin / seed env).

INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret)
VALUES
    (
        'notify',
        'telegram_bot_token',
        '{"value":""}'::jsonb,
        1,
        'Telegram Bot API token (BotFather). Required for setup-analysis getFile/sendMessage.',
        true
    ),
    (
        'notify',
        'telegram_chat_id',
        '{"value":""}'::jsonb,
        1,
        'Default chat id for generic Telegram notifications (user / group / channel). Optional for webhook-only bot flows.',
        true
    )
ON CONFLICT (module, config_key) DO NOTHING;

-- >>> merged from former 0005_enable_tbm_notifications.sql, 0006_nansen_tick_optimization.sql,
--     0007_telegram_setup_analysis_gemini_model_2_5.sql, 0007_tbm_auto_execute_config.sql
--     (single-file migrations for sqlx embed).

-- Enable TBM setup notifications via Telegram
INSERT INTO system_config (module, config_key, value) VALUES
  ('notify', 'notify_on_tbm_setup', '"true"'),
  ('notify', 'notify_on_tbm_channels', '"telegram"')
ON CONFLICT (module, config_key) DO UPDATE SET value = EXCLUDED.value;

-- Nansen API kredi optimizasyonu: tick sürelerini artırarak günlük sorgu sayısını ~81% azalt
-- Varsayılan: 300-600s → Optimize: 1800-3600s (kritik olmayan endpointler devre dışı)

INSERT INTO system_config (module, config_key, value) VALUES
  -- Core endpointler: 1 saatte bir (önceki: 5-10dk)
  ('worker', 'nansen_token_screener_tick_secs', '3600'),
  ('worker', 'nansen_netflows_tick_secs', '3600'),
  ('worker', 'nansen_perp_trades_tick_secs', '1800'),
  ('worker', 'nansen_perp_leaderboard_tick_secs', '3600'),
  ('worker', 'nansen_whale_perp_positions_tick_secs', '1800'),
  -- Düşük öncelikli endpointler: daha seyrek
  ('worker', 'nansen_holdings_tick_secs', '7200'),
  ('worker', 'nansen_flow_intel_tick_secs', '7200')
ON CONFLICT (module, config_key) DO UPDATE SET value = EXCLUDED.value;

-- Gemini: gemini-2.0-flash is not available to new API users (404). Bump default to gemini-2.5-flash.
UPDATE system_config
SET
    value = jsonb_set(value, '{value}', '"gemini-2.5-flash"'::jsonb, true),
    updated_at = now()
WHERE
    module = 'telegram_setup_analysis'
    AND config_key = 'gemini_model'
    AND trim(coalesce(value ->> 'value', '')) IN ('gemini-2.0-flash', 'gemini-2.0-flash-lite');

-- >>> merged from former 0007_tbm_auto_execute_config.sql
-- TBM auto-execute: Setup tespit edildiginde paper/live execution pipeline'a otomatik sinyal gonderimi
-- Default: kapali (guvenlik icin). Aktif edildiginde Strong+ sinyaller islem acar.
INSERT INTO system_config (module, config_key, value) VALUES
  ('worker', 'tbm_auto_execute_enabled', '"false"'),
  ('worker', 'tbm_execute_min_signal', '"Strong"')
ON CONFLICT (module, config_key) DO UPDATE SET value = EXCLUDED.value;

-- TBM auto-execute + Telegram notify defaults (upsert after baseline seeds).
UPDATE system_config
SET value = '"true"'::jsonb
WHERE module = 'worker'
  AND config_key = 'tbm_auto_execute_enabled';

UPDATE system_config
SET value = '"Moderate"'::jsonb
WHERE module = 'worker'
  AND config_key = 'tbm_execute_min_signal';

INSERT INTO system_config (module, config_key, value) VALUES
  ('worker', 'tbm_auto_execute_enabled', '"true"'),
  ('worker', 'tbm_execute_min_signal', '"Moderate"'),
  ('notify', 'notify_on_tbm_setup', '"true"'),
  ('notify', 'notify_on_tbm_channels', '"telegram"')
ON CONFLICT (module, config_key) DO UPDATE SET value = EXCLUDED.value;

