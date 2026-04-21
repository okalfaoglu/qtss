-- Seed the swing-prominence filter threshold.
--
-- `min_prominence_pct` is the minimum fractional price change between
-- two adjacent zigzag pivots required for both to survive the filter.
-- Pairs below it get absorbed into the surrounding swing so the chart
-- isn't littered with sub-percent noise pivots (CLAUDE.md #2 — all
-- thresholds live in system_config, not in code).
--
-- Default 0.003 (0.3%) matches TradingView's default visual density on
-- intraday crypto. Operators tune via the GUI Config Editor.

INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('zigzag', 'min_prominence_pct',
        '{"pct": 0.003}'::jsonb,
        'Drop zigzag pivot pairs whose |Δprice|/prev_price is below this fraction (0.003 = 0.3%). Reduces noise in sideways ranges.')
ON CONFLICT (module, config_key) DO NOTHING;
