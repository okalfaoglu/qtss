-- Pine ACP v6 fabrika: 4 zigzag (useZigzag1 açık), pivot_tail_skip_max=0, last_pivot both (lastPivotDirection=both).
-- Not: Dosya adı tarihsel (önceki sürüm 7 Fib idi); yeni kurulumlarda 0007 ile aynı hedef.

UPDATE app_config
SET value = jsonb_set(
  jsonb_set(
    jsonb_set(
      value,
      '{zigzag}',
      '[
        {"enabled": true, "length": 8, "depth": 55},
        {"enabled": false, "length": 13, "depth": 34},
        {"enabled": false, "length": 21, "depth": 21},
        {"enabled": false, "length": 34, "depth": 13}
      ]'::jsonb
    ),
    '{scanning}',
    coalesce(value->'scanning', '{}'::jsonb) || '{"pivot_tail_skip_max": 0}'::jsonb
  ),
  '{patterns}',
  '{
    "1": {"enabled": true, "last_pivot": "both"},
    "2": {"enabled": true, "last_pivot": "both"},
    "3": {"enabled": true, "last_pivot": "both"},
    "4": {"enabled": true, "last_pivot": "both"},
    "5": {"enabled": true, "last_pivot": "both"},
    "6": {"enabled": true, "last_pivot": "both"},
    "7": {"enabled": true, "last_pivot": "both"},
    "8": {"enabled": true, "last_pivot": "both"},
    "9": {"enabled": true, "last_pivot": "both"},
    "10": {"enabled": true, "last_pivot": "both"},
    "11": {"enabled": true, "last_pivot": "both"},
    "12": {"enabled": true, "last_pivot": "both"},
    "13": {"enabled": true, "last_pivot": "both"}
  }'::jsonb
)
WHERE key = 'acp_chart_patterns';
