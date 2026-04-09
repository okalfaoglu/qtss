-- 0029_qtss_v2_nansen_symbol_map_seed.sql
--
-- Faz 7.7 final step — seed `onchain.nansen.symbol_map` with the
-- BTC/ETH wrapped-token mappings used by the qtss-nansen worker
-- snapshots. Without this map the NansenFetcher returns
-- UnsupportedSymbol for every engine_symbol and the Chain category
-- score stays empty.
--
-- Shape (CLAUDE.md #2 — config-driven, GUI-editable later):
--   {
--     "BTCUSDT": { "chain": "ethereum", "address": "0x2260...", "symbol": "WBTC" },
--     "ETHUSDT": { "chain": "ethereum", "address": "0xC02a...", "symbol": "WETH" }
--   }
--
-- The fetcher also accepts the stripped form ("BTC", "ETH") so the
-- same entries cover spot pairs. Operators add new tokens through the
-- Config Editor — no code change required.

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
        )
    ),
    updated_at = NOW()
WHERE module = 'onchain'
  AND config_key = 'nansen.symbol_map'
  AND (value IS NULL OR value = '{}'::jsonb);
