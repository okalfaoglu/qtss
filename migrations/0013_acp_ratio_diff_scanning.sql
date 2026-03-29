-- ACP tarama: Pine `ratioDiffEnabled` / `ratioDiff` — GUI varsayılanı ile hizalı (`analysis.rs` `default_acp_chart_patterns_json`).

UPDATE app_config
SET value = jsonb_set(
    value,
    '{scanning}',
    coalesce(value->'scanning', '{}'::jsonb)
      || '{"ratio_diff_enabled": false, "ratio_diff_max": 1.0}'::jsonb,
    true
  ),
  updated_at = now()
WHERE key = 'acp_chart_patterns'
  AND (value->'scanning'->'ratio_diff_enabled' IS NULL);
