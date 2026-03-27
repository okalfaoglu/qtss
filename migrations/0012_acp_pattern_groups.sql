-- Pine `allowedPatterns` grupları (geometri / yön / dinamik); eksikse tümü açık varsayılır.

UPDATE app_config
SET value = coalesce(value, '{}'::jsonb) || '{
  "pattern_groups": {
    "geometric": { "channels": true, "wedges": true, "triangles": true },
    "direction": { "rising": true, "falling": true, "flat_bidirectional": true },
    "formation_dynamics": { "expanding": true, "contracting": true, "parallel": true }
  }
}'::jsonb
WHERE key = 'acp_chart_patterns'
  AND NOT (coalesce(value, '{}'::jsonb) ? 'pattern_groups');
