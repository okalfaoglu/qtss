-- SPEC_ONCHAIN_SIGNALS §5.5 / §4.2 — bileşen ağırlıkları (`onchain_signal_scorer`).
-- Admin: `PUT /api/v1/config` key `onchain_signal_weights`. Env: `QTSS_ONCHAIN_SIGNAL_WEIGHTS_KEY`.

INSERT INTO app_config (key, value, description)
VALUES (
    'onchain_signal_weights',
    '{
      "taker": 1.0,
      "funding": 1.0,
      "oi": 1.0,
      "ls_ratio": 1.0,
      "coinglass_netflow": 1.0,
      "coinglass_liquidations": 1.0,
      "hl_meta": 1.0,
      "nansen": 1.0
    }'::jsonb,
    'On-chain aggregate: score_i × confidence_i × weight_i / Σ(confidence_i × weight_i) — `qtss-worker/src/onchain_signal_scorer.rs`.'
)
ON CONFLICT (key) DO NOTHING;
