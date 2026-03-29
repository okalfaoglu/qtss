-- PLAN §4.1 — default regime weights (English keys). Admin may override via `PUT /api/v1/config` key `confluence_weights_by_regime`.

INSERT INTO app_config (key, value, description)
VALUES (
    'confluence_weights_by_regime',
    '{
      "range": { "technical": 0.50, "onchain": 0.35, "smart_money": 0.15 },
      "trend": { "technical": 0.30, "onchain": 0.40, "smart_money": 0.30 },
      "breakout": { "technical": 0.40, "onchain": 0.45, "smart_money": 0.15 },
      "uncertain": { "technical": 0.20, "onchain": 0.30, "smart_money": 0.50 }
    }'::jsonb,
    'Worker confluence: pillar weights per regime (`qtss-worker/src/confluence.rs`).'
)
ON CONFLICT (key) DO NOTHING;
