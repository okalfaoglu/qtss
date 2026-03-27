-- Pine `lastPivotDirection` karşılığı: varsayılan `both` (allowedLastPivotDirections = hepsi serbest).

UPDATE app_config
SET value = jsonb_set(
  coalesce(value, '{}'::jsonb),
  '{scanning}',
  coalesce(value->'scanning', '{}'::jsonb) || '{"last_pivot_direction": "both"}'::jsonb
)
WHERE key = 'acp_chart_patterns'
  AND (value->'scanning'->>'last_pivot_direction' IS NULL);
