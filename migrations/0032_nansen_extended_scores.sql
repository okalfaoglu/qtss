-- Nansen extended on-chain score columns + weights (QTSS_CURSOR_DEV_GUIDE ADIM 6, §3.2).
-- Requires existing `onchain_signal_scores` table (deploy engine/on-chain migrations first if missing).

ALTER TABLE onchain_signal_scores
    ADD COLUMN IF NOT EXISTS nansen_netflow_score DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS nansen_perp_score DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS nansen_buyer_quality_score DOUBLE PRECISION;

UPDATE app_config
SET value = value
    || '{"nansen_netflows": 1.0, "nansen_perp": 1.5, "nansen_buyer_quality": 0.8, "nansen_flow_intelligence": 1.0}'::jsonb
WHERE key = 'onchain_signal_weights';

INSERT INTO app_config (key, value, description)
VALUES (
    'nansen_whale_watchlist',
    '{"wallets": [], "last_updated": null}'::jsonb,
    'Whale wallet list filled from perp-leaderboard (worker pipeline)'
)
ON CONFLICT (key) DO NOTHING;
