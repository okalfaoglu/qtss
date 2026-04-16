-- 0113_faz9_data_infra.sql
--
-- Faz 9.0.0 — AI confluence engine, veri altyapısı ilk adım.
--
-- Kapsam:
--   1. Nansen master kill-switch (`worker.nansen_enabled=false`) — token
--      tükenmesi sonrası kalıcı kapama (docs/FAZ_9_AI_CONFLUENCE.md #8).
--   2. DefiLlama stablecoin veri kaynakları (ücretsiz) — makro likidite
--      feature'ları için (external_misc_loop otomatik çeker).
--   3. Binance `fundingRate` historical snapshot — aktif semboller için
--      worker başlangıcındaki `ensure_binance_sources_for_active_symbols`
--      tarafından otomatik eklenir; mevcut çalışan worker'ın bu yeni
--      prefix'i farketmesi için kod tarafında da prefix listesi güncellendi.
--
-- Not: WS-temelli `liquidation_stream_loop` (@forceOrder) ve
-- `aggtrade_cvd_loop` (@aggTrade) bir sonraki increment'ta eklenecek.

-- 1) Nansen master switch — config_schema + system_config.
SELECT _qtss_register_key(
    'nansen_enabled','worker','worker','bool',
    'false'::jsonb, '',
    'Master kill-switch for all Nansen loops (token_screener, netflows, holdings, perp_*, setup_scan). Set false → credit drain durur.',
    'bool', false, 'normal', ARRAY['nansen','kill_switch']);

-- DB'de zaten false; migration idempotent olsun diye yeniden yazıyoruz.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('worker','nansen_enabled','false'::jsonb,'Nansen master switch — Faz 9: token biterken kapandı, AI pipeline Nansen''a bağlı değil.')
ON CONFLICT (module, config_key) DO UPDATE SET
    value = EXCLUDED.value,
    description = EXCLUDED.description,
    updated_at = now();

-- 2) DefiLlama stablecoin kaynakları (ücretsiz, API key gerektirmez).
--    external_misc_loop prefix'i `binance_` / `coinglass_` / `hl_` değilse
--    otomatik çeker; tick_secs konservatif (stablecoin arzı saatte değişir).
INSERT INTO external_data_sources
    (key, enabled, method, url, headers_json, body_json, tick_secs, description)
VALUES
    ('defillama_stablecoins',
     true, 'GET',
     'https://stablecoins.llama.fi/stablecoins?includePrices=true',
     '{}'::jsonb, NULL, 1800,
     'DefiLlama aggregate stablecoin supply (USDT/USDC/DAI/...) — makro likidite feature'),
    ('defillama_stablecoincharts_all',
     true, 'GET',
     'https://stablecoins.llama.fi/stablecoincharts/all',
     '{}'::jsonb, NULL, 3600,
     'DefiLlama historical stablecoin supply chart (all chains aggregate)')
ON CONFLICT (key) DO UPDATE SET
    url = EXCLUDED.url,
    enabled = EXCLUDED.enabled,
    tick_secs = EXCLUDED.tick_secs,
    description = EXCLUDED.description,
    updated_at = now();

-- 3) DefiLlama feature'ları AI pipeline tarafında kullanılacağı için
--    flag'i config'e bağla (CLAUDE.md #2). Default on — ücretsiz API.
SELECT _qtss_register_key(
    'defillama.enabled','ingest','ingest','bool',
    'true'::jsonb, '',
    'Toggle DefiLlama stablecoin fetch (free, no API key). Kapatırsan external_misc_loop atlar (via external_data_sources.enabled=false).',
    'bool', false, 'normal', ARRAY['defillama','macro']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('ingest','defillama.enabled','true'::jsonb,'DefiLlama stablecoin ingest switch.')
ON CONFLICT (module, config_key) DO NOTHING;

-- 4) Binance funding rate — config schema registration (dokümantasyon amaçlı;
--    row ekleme worker boot'ta `ensure_binance_sources_for_active_symbols`
--    tarafından yapılır, prefix: `binance_funding_rate_<pair>`).
SELECT _qtss_register_key(
    'binance.funding_rate.enabled','ingest','ingest','bool',
    'true'::jsonb, '',
    'Binance historical funding rate fetch (fapi/v1/fundingRate). Worker start-up''da aktif semboller için otomatik row oluşturur.',
    'bool', false, 'normal', ARRAY['binance','derivatives','funding']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('ingest','binance.funding_rate.enabled','true'::jsonb,'Binance funding rate ingest switch.')
ON CONFLICT (module, config_key) DO NOTHING;
