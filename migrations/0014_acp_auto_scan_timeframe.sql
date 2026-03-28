-- Web: `scanning.auto_scan_on_timeframe_change` — varsayılan kapalı.

UPDATE app_config
SET value = jsonb_set(
  coalesce(value, '{}'::jsonb),
  '{scanning}',
  coalesce(value->'scanning', '{}'::jsonb)
    || '{"auto_scan_on_timeframe_change": false}'::jsonb,
  true
)
WHERE key = 'acp_chart_patterns';
