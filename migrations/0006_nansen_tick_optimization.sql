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
