-- Derivatives signal config (Faz 12B).
-- Activates the `DerivativesWriter` in qtss-engine and seeds
-- thresholds for the five derivatives-flow detectors.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('derivatives', 'enabled',   '{"enabled": true}'::jsonb,
     'Master on/off for the derivatives-signal engine writer.'),
    ('derivatives', 'min_score', '{"score": 0.55}'::jsonb,
     'Minimum event score (0..1) before a derivatives event is persisted.'),

    -- FundingSpike (z-score on rolling funding window).
    ('derivatives', 'thresholds.funding_z',      '{"value": 2.0}'::jsonb,
     'Funding-rate z-score threshold (|z| ≥ this triggers a spike).'),
    ('derivatives', 'thresholds.funding_window', '{"value": 21}'::jsonb,
     'Number of prior funding periods used for z-score baseline (21 ≈ 7 days @ 8h cycles).'),

    -- OIImbalance.
    ('derivatives', 'thresholds.oi_delta_pct',             '{"value": 0.05}'::jsonb,
     'Open-interest change threshold as fraction of baseline (0.05 = 5%).'),
    ('derivatives', 'thresholds.oi_price_divergence_pct',  '{"value": 0.02}'::jsonb,
     'Price-move threshold for an OI imbalance to qualify as a divergence.'),

    -- BasisDislocation.
    ('derivatives', 'thresholds.basis_dislocation_pct', '{"value": 0.001}'::jsonb,
     'Perp-spot basis threshold as fraction of index price (0.001 = 10 bps).'),

    -- Long/Short ratio extremes.
    ('derivatives', 'thresholds.lsr_long_extreme',  '{"value": 2.5}'::jsonb,
     'Long/Short ratio above this counts as extreme-long (contrarian bearish).'),
    ('derivatives', 'thresholds.lsr_short_extreme', '{"value": 2.5}'::jsonb,
     'Short-extreme: trigger when ratio <= 1/lsr_short_extreme (crowded short → contrarian bullish).'),

    -- Taker flow extremes.
    ('derivatives', 'thresholds.taker_buy_dominance',  '{"value": 1.4}'::jsonb,
     'Taker buy/sell ratio above this counts as buyer-dominant.'),
    ('derivatives', 'thresholds.taker_sell_dominance', '{"value": 1.4}'::jsonb,
     'Seller-dominant: trigger when ratio <= 1/taker_sell_dominance.')
ON CONFLICT (module, config_key) DO NOTHING;
