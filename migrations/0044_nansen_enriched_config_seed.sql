-- 0044: Config seed for Nansen enriched analyzers + chain expansion.

INSERT INTO system_config (module, config_key, value, description)
VALUES
  -- Chain expansion (was hardcoded ["ethereum"])
  ('worker',  'nansen.smart_money_chains',              '["all"]',  'Chains sent to Nansen smart-money endpoints (["all"] or specific list)'),

  -- Enriched master switch
  ('onchain', 'nansen.enriched.enabled',                'true',     'Enable enriched Nansen analyzers (cross-chain, DEX spike, whale)'),

  -- Cross-chain flow
  ('onchain', 'nansen.enriched.cross_chain.min_chains',     '2',    'Min chains agreeing for cross-chain signal'),
  ('onchain', 'nansen.enriched.cross_chain.agreement_boost','0.3',  'Confidence boost when multiple chains agree'),

  -- DEX volume spike
  ('onchain', 'nansen.enriched.dex_spike.threshold_x',      '3.0',  'Volume spike multiplier vs baseline'),
  ('onchain', 'nansen.enriched.dex_spike.min_value_usd',    '50000', 'Min USD volume to consider a spike'),

  -- Whale concentration
  ('onchain', 'nansen.enriched.whale.top_n',                '10',   'Top N wallets to track for concentration'),
  ('onchain', 'nansen.enriched.whale.delta_threshold',      '0.05', 'Concentration change trigger (5%)'),

  -- Alert settings
  ('onchain', 'nansen.enriched.alert_cooldown_s',           '3600', 'Dedup window for enriched alerts (1h)'),
  ('onchain', 'nansen.enriched.alert_channels',             '"telegram"', 'Notification channels for enriched alerts'),

  -- Blend weights (added to existing NansenTuning)
  ('onchain', 'nansen.enriched.weight.cross_chain',         '0.15', 'Blend weight for cross-chain signal'),
  ('onchain', 'nansen.enriched.weight.dex_spike',           '0.10', 'Blend weight for DEX spike signal'),
  ('onchain', 'nansen.enriched.weight.whale_conc',          '0.10', 'Blend weight for whale concentration'),

  -- Raw data retention
  ('onchain', 'nansen.raw_flows.retention_days',            '90',   'Days to keep nansen_raw_flows rows (0 = forever)')
ON CONFLICT (module, config_key) DO NOTHING;
