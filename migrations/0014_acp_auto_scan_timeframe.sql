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
