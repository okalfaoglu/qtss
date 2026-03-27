-- Pine: Auto Chart Patterns [Trendoscope®] v6 — useZigzag1..4 (yalnız z1 açık), lastPivot both, ScanProperties.offset=0.

INSERT INTO app_config (key, value, description)
VALUES (
    'acp_chart_patterns',
    '{
      "version": 1,
      "ohlc": { "open": "open", "high": "high", "low": "low", "close": "close" },
      "zigzag": [
        { "enabled": true, "length": 8, "depth": 55 },
        { "enabled": false, "length": 13, "depth": 34 },
        { "enabled": false, "length": 21, "depth": 21 },
        { "enabled": false, "length": 34, "depth": 13 }
      ],
      "scanning": {
        "number_of_pivots": 5,
        "error_threshold_percent": 20,
        "flat_threshold_percent": 20,
        "verify_bar_ratio": true,
        "bar_ratio_limit": 0.382,
        "avoid_overlap": true,
        "repaint": false,
        "pivot_tail_skip_max": 0,
        "max_zigzag_levels": 2,
        "upper_direction": 1,
        "lower_direction": -1,
        "ignore_if_entry_crossed": false,
        "size_filters": {
          "filter_by_bar": false,
          "min_pattern_bars": 0,
          "max_pattern_bars": 1000,
          "filter_by_percent": false,
          "min_pattern_percent": 0,
          "max_pattern_percent": 100
        }
      },
      "patterns": {
        "1": { "enabled": true, "last_pivot": "both" },
        "2": { "enabled": true, "last_pivot": "both" },
        "3": { "enabled": true, "last_pivot": "both" },
        "4": { "enabled": true, "last_pivot": "both" },
        "5": { "enabled": true, "last_pivot": "both" },
        "6": { "enabled": true, "last_pivot": "both" },
        "7": { "enabled": true, "last_pivot": "both" },
        "8": { "enabled": true, "last_pivot": "both" },
        "9": { "enabled": true, "last_pivot": "both" },
        "10": { "enabled": true, "last_pivot": "both" },
        "11": { "enabled": true, "last_pivot": "both" },
        "12": { "enabled": true, "last_pivot": "both" },
        "13": { "enabled": true, "last_pivot": "both" }
      },
      "display": {
        "theme": "dark",
        "pattern_line_width": 2,
        "zigzag_line_width": 1,
        "show_pattern_label": true,
        "show_pivot_labels": true,
        "show_zigzag": true,
        "max_patterns": 20
      },
      "calculated_bars": 5000
    }'::jsonb,
    'ACP [Trendoscope] — TV göstergesi ile hizalı zigzag / tarama / desen filtreleri (GUI + kanal taraması).'
)
ON CONFLICT (key) DO NOTHING;
