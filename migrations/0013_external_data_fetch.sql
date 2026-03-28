-- Genel HTTP kaynakları: worker periyodik çeker, ham JSON `external_data_snapshots` içinde tutulur.
-- Yeni kaynak eklemek için satır ekleyin (restart gerekmez; bir sonraki tick'te çekilir).

CREATE TABLE external_data_sources (
    key TEXT PRIMARY KEY CHECK (key ~ '^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$'),
    enabled BOOLEAN NOT NULL DEFAULT true,
    method TEXT NOT NULL DEFAULT 'GET' CHECK (upper(btrim(method)) IN ('GET', 'POST')),
    url TEXT NOT NULL,
    headers_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    body_json JSONB,
    tick_secs INT NOT NULL DEFAULT 300 CHECK (tick_secs >= 30),
    description TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE external_data_snapshots (
    source_key TEXT PRIMARY KEY REFERENCES external_data_sources (key) ON DELETE CASCADE,
    request_json JSONB NOT NULL,
    response_json JSONB,
    status_code SMALLINT,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);

CREATE INDEX idx_external_data_snapshots_computed_at ON external_data_snapshots (computed_at DESC);

COMMENT ON TABLE external_data_sources IS 'qtss-worker external_fetch_loop: GET/POST URL + tick_secs';
COMMENT ON TABLE external_data_snapshots IS 'Son başarılı/başarısız yanıt; GET /api/v1/analysis/external-fetch/snapshots/{key}';

-- Örnek (isteğe bağlı — yorumu kaldırıp çalıştırın):
-- INSERT INTO external_data_sources (key, method, url, body_json, tick_secs, description) VALUES
-- ('hyperliquid_meta', 'POST', 'https://api.hyperliquid.xyz/info',
--  '{"type":"metaAndAssetCtxs"}'::jsonb, 60, 'HL perp meta + asset ctx'),
-- ('defillama_dex_overview', 'GET',
--  'https://api.llama.fi/overview/dexs?excludeTotalDataChart=true&dataType=dailyVolume',
--  NULL, 600, 'DeFi Llama DEX overview');
