-- Pine abstractchartpatterns: ScanProperties.ignoreIfEntryCrossed + SizeFilters (yalnız eksikse eklenir).

UPDATE app_config
SET value = jsonb_set(
  value,
  '{scanning}',
  coalesce(value->'scanning', '{}'::jsonb)
    || CASE
      WHEN (value->'scanning' ? 'ignore_if_entry_crossed') THEN '{}'::jsonb
      ELSE '{"ignore_if_entry_crossed": false}'::jsonb
    END
    || CASE
      WHEN (value->'scanning' ? 'size_filters') THEN '{}'::jsonb
      ELSE '{
        "size_filters": {
          "filter_by_bar": false,
          "min_pattern_bars": 0,
          "max_pattern_bars": 1000,
          "filter_by_percent": false,
          "min_pattern_percent": 0,
          "max_pattern_percent": 100
        }
      }'::jsonb
    END
)
WHERE key = 'acp_chart_patterns';
