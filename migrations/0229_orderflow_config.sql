-- Order-flow detector config (Faz 12C).
-- Activates `OrderFlowWriter` (qtss-engine) — LiquidationCluster /
-- BlockTrade / CVDDivergence.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('orderflow', 'enabled',   '{"enabled": true}'::jsonb,
     'Master on/off for the order-flow engine writer.'),
    ('orderflow', 'min_score', '{"score": 0.55}'::jsonb,
     'Minimum event score (0..1) before an order-flow event is persisted.'),

    -- Liquidation cluster.
    ('orderflow', 'thresholds.liq_cluster_min_count',        '{"value": 15}'::jsonb,
     'Minimum number of liquidation events in the window for a cluster to qualify.'),
    ('orderflow', 'thresholds.liq_cluster_min_notional_usd', '{"value": 250000}'::jsonb,
     'Minimum total cluster notional (USD).'),
    ('orderflow', 'thresholds.liq_cluster_window_secs',      '{"value": 3600}'::jsonb,
     'Window (seconds) for the liquidation scan.'),

    -- Block trade.
    ('orderflow', 'thresholds.block_trade_notional_usd', '{"value": 500000}'::jsonb,
     'Single liquidation notional threshold (USD) for a block-trade event.'),

    -- CVD divergence.
    ('orderflow', 'thresholds.cvd_divergence_bars',              '{"value": 12}'::jsonb,
     'Bar lookback for CVD-vs-price comparison.'),
    ('orderflow', 'thresholds.cvd_divergence_price_min_pct',     '{"value": 0.01}'::jsonb,
     'Minimum |price move| (fraction) over the lookback for a divergence to qualify.'),
    ('orderflow', 'thresholds.cvd_divergence_cvd_opposite_min',  '{"value": 0.15}'::jsonb,
     'CVD counter-move magnitude as fraction of window abs-sum for divergence to trigger.')
ON CONFLICT (module, config_key) DO NOTHING;
