-- 0036_qtss_nansen_chain_pillar_revival.sql
--
-- Onchain chain-pillar revival (master gap list, 2026-04-10).
--
-- Symptom: qtss_v2_onchain_metrics rows show chain_score = 0 / NULL
-- across the board. The NansenFetcher (qtss-onchain/src/nansen.rs) reads
-- four `data_snapshots` keys and blends them into the chain category:
--   nansen_netflows           ← nansen_netflows_loop           (default ON)
--   nansen_holdings           ← nansen_holdings_loop           (default ON)
--   nansen_flow_intelligence  ← nansen_flow_intel_loop         (default ON)
--   nansen_smart_money_dex_trades ← nansen_smart_money_dex_trades_loop (OPT-IN)
--
-- Two root causes blocking all four:
--   1) Master switch `worker.nansen_enabled` was set to false back when
--      Nansen credits ran out — every loop short-circuits at the gate.
--      Credits are restored; Setup Engine + chain pillar both need this on.
--   2) `nansen_smart_money_dex_trades_loop` is opt-in (default off) so
--      even when the master switch flips back on, this snapshot key
--      stays empty → fetcher's dex-trades component never contributes.
--
-- The other two missing-in-prod keys (nansen_flow_intelligence,
-- nansen_smart_money_dex_trades) were the "2 eksik snapshot key" item
-- in the master gap list. flow_intelligence already defaults ON, so
-- once the master switch flips it will start writing on its own tick.
--
-- Idempotent: ON CONFLICT overwrite.

INSERT INTO system_config (module, config_key, value, updated_at)
VALUES ('worker', 'nansen_enabled', '{"enabled": true}'::jsonb, NOW())
ON CONFLICT (module, config_key) DO UPDATE
   SET value = '{"enabled": true}'::jsonb,
       updated_at = NOW();

INSERT INTO system_config (module, config_key, value, updated_at)
VALUES ('worker', 'nansen_loop_smart_money_dex_trades_enabled',
        '{"enabled": true}'::jsonb, NOW())
ON CONFLICT (module, config_key) DO UPDATE
   SET value = '{"enabled": true}'::jsonb,
       updated_at = NOW();

-- Sanity belt: explicitly pin the three default-on loops the chain
-- pillar depends on, so a future operator can't half-disable the
-- pillar by clearing only the master switch.
INSERT INTO system_config (module, config_key, value, updated_at)
VALUES
  ('worker', 'nansen_loop_netflows_enabled',           '{"enabled": true}'::jsonb, NOW()),
  ('worker', 'nansen_loop_holdings_enabled',           '{"enabled": true}'::jsonb, NOW()),
  ('worker', 'nansen_loop_flow_intelligence_enabled',  '{"enabled": true}'::jsonb, NOW())
ON CONFLICT (module, config_key) DO UPDATE
   SET value = '{"enabled": true}'::jsonb,
       updated_at = NOW();
