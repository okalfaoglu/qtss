-- Pine `ratioDiffEnabled` / `ratioDiff` — kanal taraması (`SixPivotScanParams`) ile hizalı varsayılanlar.

UPDATE app_config
SET value = jsonb_set(
  coalesce(value, '{}'::jsonb),
  '{scanning}',
  coalesce(value->'scanning', '{}'::jsonb)
    || '{"ratio_diff_enabled": false, "ratio_diff_max": 1.0}'::jsonb,
  true
)
WHERE key = 'acp_chart_patterns';
