-- 0034_qtss_v2_onchain_chain_pillar_enable.sql
--
-- Post-Faz 8.0 backlog item #2 — onchain Chain pillar wakeup.
--
-- Symptom (prod 2026-04-10): /v2/onchain ekranında chain="n/a" tüm
-- semboller için. Root cause iki katmanlı:
--   1) `fetcher.nansen.enabled` migration 0028'de default false geliyor;
--      ücretli Glassnode/CryptoQuant da off olduğu için Chain kategorisi
--      hiçbir reading üretmiyor → DB'de chain_score NULL → frontend "n/a".
--   2) Migration 0029 sadece BTCUSDT + ETHUSDT'yi `onchain.nansen.symbol_map`
--      içine seed'lemiş. ADA/AVAX/BNB/SOL gibi top altcoinler için
--      NansenFetcher.resolve_symbol() None döner → UnsupportedSymbol.
--
-- Bu migration ikisini de düzeltir:
--   • `fetcher.nansen.enabled` → true (data_snapshots'ı qtss-nansen worker
--     hidrate ettiği için ekstra API key gerekmiyor; ücretsiz)
--   • `onchain.nansen.symbol_map` → top-15 sembol genişletmesi
--
-- Symbol-only entries (`{ "symbol": "X" }`) NansenFetcher.matches_row()'da
-- token_symbol fallback'i ile eşleşir. ETH bazlı LINK/UNI için address
-- alanı da set edilir (kesin eşleşme). Geri kalanları (ADA/SOL/AVAX/BNB
-- vb.) Nansen kendi native chain'lerinde sembol bazlı tanır.
--
-- Idempotent değil — `value = ...` ile mevcut map'i overwrite eder. Eğer
-- operatör Config Editor'den daha geniş bir map yazdıysa, sadece BTC/ETH
-- içeren default'tan büyükse dokunulmaz (jsonb anahtar sayısı kontrolü).

-- Adım 1: nansen fetcher'ı aç (migration 0028:117 false default'una karşı)
UPDATE system_config
SET value = 'true'::jsonb,
    updated_at = NOW()
WHERE module = 'onchain'
  AND config_key = 'fetcher.nansen.enabled'
  AND value = 'false'::jsonb;

-- Adım 2: nansen symbol map'ini genişlet — sadece henüz operatör eli
-- değmemişse (≤2 anahtar = default BTC/ETH veya boş)
UPDATE system_config
SET value = jsonb_build_object(
        'BTCUSDT', jsonb_build_object(
            'chain',   'ethereum',
            'address', '0x2260fac5e5542a773aa44fbcfedf7c193bc2c599',
            'symbol',  'WBTC'
        ),
        'ETHUSDT', jsonb_build_object(
            'chain',   'ethereum',
            'address', '0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2',
            'symbol',  'WETH'
        ),
        'SOLUSDT',  jsonb_build_object('symbol', 'SOL'),
        'BNBUSDT',  jsonb_build_object('symbol', 'BNB'),
        'XRPUSDT',  jsonb_build_object('symbol', 'XRP'),
        'ADAUSDT',  jsonb_build_object('symbol', 'ADA'),
        'AVAXUSDT', jsonb_build_object('symbol', 'AVAX'),
        'DOGEUSDT', jsonb_build_object('symbol', 'DOGE'),
        'TRXUSDT',  jsonb_build_object('symbol', 'TRX'),
        'DOTUSDT',  jsonb_build_object('symbol', 'DOT'),
        'MATICUSDT', jsonb_build_object('symbol', 'MATIC'),
        'POLUSDT',  jsonb_build_object('symbol', 'POL'),
        'LINKUSDT', jsonb_build_object(
            'chain',   'ethereum',
            'address', '0x514910771af9ca656af840dff83e8264ecf986ca',
            'symbol',  'LINK'
        ),
        'UNIUSDT',  jsonb_build_object(
            'chain',   'ethereum',
            'address', '0x1f9840a85d5af5bf1d1762f925bdaddc4201f984',
            'symbol',  'UNI'
        ),
        'LTCUSDT',  jsonb_build_object('symbol', 'LTC')
    ),
    updated_at = NOW()
WHERE module = 'onchain'
  AND config_key = 'nansen.symbol_map'
  AND (value IS NULL
       OR value = '{}'::jsonb
       OR jsonb_typeof(value) = 'object' AND (SELECT count(*) FROM jsonb_object_keys(value)) <= 2);

-- Not: master switch `onchain.enabled` durumu prod'da zaten true (rows
-- yazılmaya devam ediyor — derivatives = 0.00, stablecoin = 0.50 veriliyor).
-- Eğer false ise operatör Config Editor'den manuel açar; bu migration
-- runtime davranışını değiştirmez.
